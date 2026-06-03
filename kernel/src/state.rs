use alloc::vec::Vec;

use crate::{
    cap::{
        CapabilityDescriptor, CapabilitySpace, MintParams, ObjectId, ReplyCap, RetypeDestination,
        RetypeResult, RetypeTarget, Rights, SlotId,
    },
    invocation::{Invocation, InvocationError, InvocationOutcome, invoke},
    ipc::{Endpoint, IpcPayload, IpcReceiveOptions, IpcSendOptions},
    notification::{BoundTcbSignal, Notification},
    object::{FrameObject, KernelObjectKind, KernelObjectRef, ObjectTable, ObjectTableError},
    reply::ReplyState,
    scheduler::{Scheduler, SchedulerError},
    tcb::{CpuId, Tcb, ThreadId, ThreadState},
    thread_action::{
        ReceiveIpcRequest, SendIpcRequest, ThreadAction, ThreadActionError, ThreadTable,
        poll_notification, recv_ipc, reply_to_caller, resume_tcb, send_ipc, signal_notification,
        wait_notification,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationContext {
    current: ThreadId,
    cpu: CpuId,
    payload: IpcPayload,
    reply: Option<ObjectId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionOutcome {
    Thread(ThreadAction),
    ThreadWithReplyCap {
        thread: ThreadAction,
        reply: CapabilityDescriptor,
    },
    Capability {
        descriptor: CapabilityDescriptor,
    },
    CapabilityMutation,
    Retyped {
        descriptors: Vec<CapabilityDescriptor>,
    },
    Unsupported(UnsupportedInvocation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedInvocation {
    FrameMap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelExecutionError {
    Invocation(InvocationError),
    Object(ObjectTableError),
    Thread(ThreadActionError),
    MissingReplyObject { endpoint: ObjectId },
    ReplyObjectMustBeDistinct { endpoint: ObjectId, reply: ObjectId },
    ReplyAuthorityMismatch { reply: ObjectId },
    ThreadAlreadyExists { thread: ThreadId },
    Scheduler(SchedulerError),
}

#[derive(Debug)]
pub struct KernelState {
    cspace: CapabilitySpace,
    objects: ObjectTable,
    threads: ThreadTable,
    scheduler: Scheduler,
}

impl InvocationContext {
    pub const fn new(current: ThreadId, cpu: CpuId) -> Self {
        Self {
            current,
            cpu,
            payload: IpcPayload::empty(),
            reply: None,
        }
    }

    pub const fn current(self) -> ThreadId {
        self.current
    }

    pub const fn cpu(self) -> CpuId {
        self.cpu
    }

    pub const fn payload(self) -> IpcPayload {
        self.payload
    }

    pub const fn reply(self) -> Option<ObjectId> {
        self.reply
    }

    pub const fn with_payload(mut self, payload: IpcPayload) -> Self {
        self.payload = payload;
        self
    }

    pub const fn with_reply(mut self, reply: ObjectId) -> Self {
        self.reply = Some(reply);
        self
    }
}

impl KernelState {
    pub fn new(cpus: &[CpuId]) -> Result<Self, KernelExecutionError> {
        Ok(Self {
            cspace: CapabilitySpace::new(),
            objects: ObjectTable::new(),
            threads: ThreadTable::new(),
            scheduler: Scheduler::new(cpus)?,
        })
    }

    pub fn from_parts(
        cspace: CapabilitySpace,
        objects: ObjectTable,
        threads: ThreadTable,
        scheduler: Scheduler,
    ) -> Self {
        Self {
            cspace,
            objects,
            threads,
            scheduler,
        }
    }

    pub const fn cspace(&self) -> &CapabilitySpace {
        &self.cspace
    }

    pub fn cspace_mut(&mut self) -> &mut CapabilitySpace {
        &mut self.cspace
    }

    pub const fn objects(&self) -> &ObjectTable {
        &self.objects
    }

    pub fn objects_mut(&mut self) -> &mut ObjectTable {
        &mut self.objects
    }

    pub const fn threads(&self) -> &ThreadTable {
        &self.threads
    }

    pub fn threads_mut(&mut self) -> &mut ThreadTable {
        &mut self.threads
    }

    pub const fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    pub fn scheduler_mut(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }

    pub fn insert_thread_object(
        &mut self,
        object: ObjectId,
        tcb: Tcb,
    ) -> Result<(), KernelExecutionError> {
        let thread = tcb.id();
        if self.threads.get(thread).is_some() {
            return Err(KernelExecutionError::ThreadAlreadyExists { thread });
        }
        self.scheduler.run_queue(tcb.affinity())?;
        self.objects.bind_tcb(object, thread)?;
        self.threads.insert(tcb);
        Ok(())
    }

    pub fn execute_invocation(
        &mut self,
        context: InvocationContext,
        descriptor: CapabilityDescriptor,
        invocation: Invocation,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        let outcome = invoke(&self.cspace, descriptor, invocation)?;
        self.execute_authorized(context, descriptor, outcome)
    }

    fn execute_authorized(
        &mut self,
        context: InvocationContext,
        descriptor: CapabilityDescriptor,
        outcome: InvocationOutcome,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        match outcome {
            InvocationOutcome::SendIpcAuthorized {
                endpoint,
                badge,
                blocking,
                is_call,
                can_grant,
                can_grant_reply,
                ..
            } => {
                let options = if is_call {
                    IpcSendOptions::call(blocking, can_grant, can_grant_reply)
                } else {
                    IpcSendOptions::send(blocking, can_grant, can_grant_reply)
                };
                self.execute_endpoint_send(context, endpoint, badge, options)
            }
            InvocationOutcome::ReceiveIpcAuthorized {
                endpoint,
                blocking,
                can_grant,
            } => self.execute_endpoint_recv(
                context,
                endpoint,
                IpcReceiveOptions::new(blocking, can_grant),
            ),
            InvocationOutcome::NotificationSignalAuthorized {
                notification,
                badge,
            } => self.execute_notification_signal(notification, badge),
            InvocationOutcome::NotificationReceiveAuthorized {
                notification,
                blocking,
            } => self.execute_notification_wait(context, notification, blocking),
            InvocationOutcome::ReplyAuthorized {
                reply,
                caller,
                target,
                can_grant,
            } => self.execute_reply(descriptor, reply, caller, target, can_grant),
            InvocationOutcome::FrameMapAuthorized { .. } => Ok(ExecutionOutcome::Unsupported(
                UnsupportedInvocation::FrameMap,
            )),
            InvocationOutcome::UntypedRetypeAuthorized {
                target,
                destination,
                ..
            } => self.execute_untyped_retype(descriptor, target, destination),
            InvocationOutcome::CNodeCopyAuthorized {
                source,
                destination,
                requested_rights,
            } => self.execute_cnode_copy(source, destination, requested_rights),
            InvocationOutcome::CNodeMintAuthorized {
                source,
                destination,
                requested_rights,
                params,
            } => self.execute_cnode_mint(source, destination, requested_rights, params),
            InvocationOutcome::CNodeMoveAuthorized {
                source,
                destination,
            } => self.execute_cnode_move(source, destination),
            InvocationOutcome::CNodeDeleteAuthorized { target } => {
                self.execute_cnode_delete(target)
            }
            InvocationOutcome::CNodeRevokeAuthorized { target } => {
                self.execute_cnode_revoke(target)
            }
            InvocationOutcome::TcbResumeAuthorized { tcb } => self.execute_tcb_resume(tcb),
            InvocationOutcome::TcbConfigureAuthorized {
                tcb,
                thread,
                affinity,
            } => self.execute_tcb_configure(tcb, thread, affinity),
        }
    }

    fn execute_untyped_retype(
        &mut self,
        source: CapabilityDescriptor,
        target: RetypeTarget,
        destination: Option<RetypeDestination>,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        let objects = match destination {
            Some(destination) => self
                .cspace
                .preview_retype_untyped_into(source, &target, destination)
                .map_err(InvocationError::Cap)?,
            None => alloc::vec![
                self.cspace
                    .preview_retype_untyped(source, &target)
                    .map_err(InvocationError::Cap)?
            ],
        };

        self.validate_retype_runtime_destinations(&target, &objects)?;

        let retype_result = match destination {
            Some(destination) => self
                .cspace
                .retype_untyped_into(source, target.clone(), destination)
                .map_err(InvocationError::Cap)?,
            None => {
                let descriptor = self
                    .cspace
                    .retype_untyped(source, target.clone())
                    .expect("prevalidated untyped retype must succeed");
                let object = self
                    .cspace
                    .lookup(descriptor)
                    .map_err(InvocationError::Cap)?
                    .object;
                RetypeResult {
                    descriptors: alloc::vec![descriptor],
                    objects: alloc::vec![object],
                }
            }
        };

        assert_eq!(
            objects, retype_result.objects,
            "retype preview must match committed CSpace objects"
        );
        match target {
            RetypeTarget::Endpoint => {
                for object in &retype_result.objects {
                    self.objects
                        .insert_endpoint(*object, Endpoint::new())
                        .expect("prevalidated endpoint object insertion must succeed");
                }
            }
            RetypeTarget::Frame { .. } => {
                for object in &retype_result.objects {
                    self.objects
                        .insert_frame(*object, FrameObject::new(target.minimum_size_bits()))
                        .expect("prevalidated frame object insertion must succeed");
                }
            }
            RetypeTarget::CNode { .. } => {
                for object in &retype_result.objects {
                    self.objects
                        .insert_cnode(*object)
                        .expect("prevalidated CNode object insertion must succeed");
                }
            }
            RetypeTarget::Notification => {
                for object in &retype_result.objects {
                    self.objects
                        .insert_notification(*object, Notification::new())
                        .expect("prevalidated notification object insertion must succeed");
                }
            }
            RetypeTarget::Tcb { .. } => {
                for object in &retype_result.objects {
                    self.objects
                        .insert_tcb(*object)
                        .expect("prevalidated TCB object insertion must succeed");
                }
            }
            RetypeTarget::Untyped { .. } => {}
        }

        Ok(ExecutionOutcome::Retyped {
            descriptors: retype_result.descriptors,
        })
    }

    fn validate_retype_runtime_destinations(
        &self,
        target: &RetypeTarget,
        objects: &[ObjectId],
    ) -> Result<(), KernelExecutionError> {
        match target {
            RetypeTarget::Endpoint
            | RetypeTarget::Frame { .. }
            | RetypeTarget::CNode { .. }
            | RetypeTarget::Notification
            | RetypeTarget::Tcb { .. } => {
                for object in objects {
                    self.objects.validate_unbound(*object)?;
                }
            }
            RetypeTarget::Untyped { .. } => {}
        }
        Ok(())
    }

    fn execute_cnode_copy(
        &mut self,
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        let descriptor = self
            .cspace
            .copy_into(source, destination, requested_rights)
            .map_err(InvocationError::Cap)?;
        Ok(ExecutionOutcome::Capability { descriptor })
    }

    fn execute_cnode_mint(
        &mut self,
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
        params: MintParams,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        let descriptor = self
            .cspace
            .mint_into(source, destination, requested_rights, params)
            .map_err(InvocationError::Cap)?;
        Ok(ExecutionOutcome::Capability { descriptor })
    }

    fn execute_cnode_move(
        &mut self,
        source: CapabilityDescriptor,
        destination: SlotId,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        let descriptor = self
            .cspace
            .move_capability_into(source, destination)
            .map_err(InvocationError::Cap)?;
        Ok(ExecutionOutcome::Capability { descriptor })
    }

    fn execute_cnode_delete(
        &mut self,
        target: CapabilityDescriptor,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        let deletion = self.cspace.delete(target).map_err(InvocationError::Cap)?;
        if let Some(object) = deletion.final_object {
            self.finalise_unreferenced_object(object);
        }
        Ok(ExecutionOutcome::CapabilityMutation)
    }

    fn execute_cnode_revoke(
        &mut self,
        target: CapabilityDescriptor,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        let revocation = self
            .cspace
            .revoke_descendants(target)
            .map_err(InvocationError::Cap)?;
        for object in revocation.revoked_objects {
            self.finalise_unreferenced_object(object);
        }
        Ok(ExecutionOutcome::CapabilityMutation)
    }

    fn finalise_unreferenced_object(&mut self, object: ObjectId) {
        let Ok(object_ref) = self.objects.get(object) else {
            return;
        };

        match object_ref {
            KernelObjectRef::Endpoint => self.finalise_endpoint(object),
            KernelObjectRef::Notification => self.finalise_notification(object),
            KernelObjectRef::Tcb { thread } => self.finalise_tcb_object(object, thread),
            KernelObjectRef::Frame { .. } | KernelObjectRef::CNode => {
                self.objects.remove_finalised(object);
            }
            KernelObjectRef::Reply => {
                self.objects.remove_finalised(object);
            }
        }
    }

    fn finalise_endpoint(&mut self, object: ObjectId) {
        let cancellation = self
            .objects
            .endpoint_mut(object)
            .expect("endpoint finalisation must target an endpoint object")
            .cancel_all();
        for waiter in cancellation
            .senders
            .into_iter()
            .chain(cancellation.receivers)
        {
            self.restart_thread(waiter.thread(), waiter.cpu());
        }
        self.objects.remove_finalised(object);
    }

    fn finalise_notification(&mut self, object: ObjectId) {
        let cancellation = self
            .objects
            .notification_mut(object)
            .expect("notification finalisation must target a notification object")
            .cancel_all();
        for waiter in cancellation.waiters {
            self.restart_thread(waiter.thread(), waiter.cpu());
        }
        if let Some(bound) = cancellation.bound_tcb {
            self.threads.unbind_notification(bound.thread());
        }
        self.objects.remove_finalised(object);
    }

    fn finalise_tcb_object(&mut self, object: ObjectId, thread: Option<ThreadId>) {
        if let Some(thread) = thread {
            self.scheduler.remove_thread(thread);
            if let Some(tcb) = self.threads.remove(thread) {
                self.cancel_tcb_runtime_state(&tcb);
            }
        }
        self.objects.remove_finalised(object);
    }

    fn cancel_tcb_runtime_state(&mut self, tcb: &Tcb) {
        match tcb.state() {
            ThreadState::BlockedOnSend { endpoint, .. }
            | ThreadState::BlockedOnReceive { endpoint, .. } => {
                if let Ok(endpoint) = self.objects.endpoint_mut(endpoint) {
                    endpoint.cancel_thread(tcb.id());
                }
            }
            ThreadState::BlockedOnNotification { notification } => {
                if let Ok(notification) = self.objects.notification_mut(notification) {
                    notification.cancel_waiter(tcb.id());
                }
            }
            ThreadState::BlockedOnReply
            | ThreadState::Inactive
            | ThreadState::Running
            | ThreadState::Restart
            | ThreadState::IdleThreadState => {}
        }

        if let Some(notification) = tcb.bound_notification()
            && let Ok(notification) = self.objects.notification_mut(notification)
        {
            notification.unbind_tcb();
        }
    }

    fn restart_thread(&mut self, thread: ThreadId, cpu: CpuId) {
        self.scheduler.remove_thread(thread);
        if self.threads.restart(thread).is_some() {
            let _ = self.scheduler.enqueue_validated(thread, cpu);
        }
    }

    fn execute_endpoint_send(
        &mut self,
        context: InvocationContext,
        endpoint: ObjectId,
        badge: u64,
        options: IpcSendOptions,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        self.objects
            .expect_kind(endpoint, KernelObjectKind::Endpoint)?;
        let waiting_receiver = self.objects.endpoint(endpoint)?.next_receiver();
        let call_creates_reply =
            options.mode.is_call() && (options.can_grant || options.can_grant_reply);
        let reply = if call_creates_reply {
            match waiting_receiver {
                Some(receiver) => self.reply_from_receiver_state(endpoint, receiver.thread())?,
                None => None,
            }
        } else {
            None
        };
        let caller_object = if reply.is_some() {
            Some(self.objects.tcb_object_for_thread(context.current())?)
        } else {
            None
        };
        let reply_cap = reply.map(|_| ReplyCap {
            caller: caller_object.expect("reply path must have caller object"),
            target: endpoint,
            can_grant: waiting_receiver
                .expect("reply path must have waiting receiver")
                .can_grant(),
        });
        if let (Some(reply), Some(reply_cap)) = (reply, reply_cap.as_ref()) {
            self.cspace
                .validate_reply_capability(reply, reply_cap)
                .map_err(InvocationError::Cap)?;
        }

        let action = match reply {
            Some(reply) => self.objects.with_endpoint_and_reply_mut(
                endpoint,
                reply,
                |endpoint_ref, reply_ref| {
                    let request = SendIpcRequest::new(
                        endpoint,
                        context.current(),
                        context.cpu(),
                        badge,
                        options,
                        context.payload(),
                    )
                    .with_caller(caller_object.expect("reply path must have caller object"));

                    send_ipc(
                        &mut self.threads,
                        &mut self.scheduler,
                        endpoint_ref,
                        Some(reply_ref),
                        request,
                    )
                },
            )??,
            None => {
                let endpoint_ref = self.objects.endpoint_mut(endpoint)?;
                let request = SendIpcRequest::new(
                    endpoint,
                    context.current(),
                    context.cpu(),
                    badge,
                    options,
                    context.payload(),
                );

                send_ipc(
                    &mut self.threads,
                    &mut self.scheduler,
                    endpoint_ref,
                    None,
                    request,
                )?
            }
        };

        match (reply, reply_cap) {
            (Some(reply), Some(reply_cap)) => {
                let descriptor = self
                    .cspace
                    .insert_reply_capability(reply, reply_cap)
                    .expect("prevalidated reply cap installation must succeed");
                Ok(ExecutionOutcome::ThreadWithReplyCap {
                    thread: action,
                    reply: descriptor,
                })
            }
            _ => Ok(ExecutionOutcome::Thread(action)),
        }
    }

    fn execute_endpoint_recv(
        &mut self,
        context: InvocationContext,
        endpoint: ObjectId,
        options: IpcReceiveOptions,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        self.objects
            .expect_kind(endpoint, KernelObjectKind::Endpoint)?;
        let queued_call_creates_reply =
            self.objects
                .endpoint(endpoint)?
                .next_sender()
                .is_some_and(|message| {
                    message.is_call() && (message.can_grant() || message.can_grant_reply())
                });
        let reply =
            self.reply_for_endpoint(endpoint, context.reply(), queued_call_creates_reply)?;
        let caller_object = if queued_call_creates_reply {
            let message = self
                .objects
                .endpoint(endpoint)?
                .next_sender()
                .expect("queued call precheck requires a queued sender");
            Some(self.objects.tcb_object_for_thread(message.sender())?)
        } else {
            None
        };
        let reply_cap = if let (Some(reply), Some(caller)) = (reply, caller_object) {
            let reply_cap = ReplyCap {
                caller,
                target: endpoint,
                can_grant: options.can_grant,
            };
            self.cspace
                .validate_reply_capability(reply, &reply_cap)
                .map_err(InvocationError::Cap)?;
            Some(reply_cap)
        } else {
            None
        };

        let action = match reply {
            Some(reply) => self.objects.with_endpoint_and_reply_mut(
                endpoint,
                reply,
                |endpoint_ref, reply_ref| {
                    let mut request =
                        ReceiveIpcRequest::new(endpoint, context.current(), context.cpu(), options)
                            .with_receiver_reply(
                                context
                                    .reply()
                                    .expect("reply path must provide receiver reply object"),
                            );
                    if let Some(caller_object) = caller_object {
                        request = request.with_caller(caller_object);
                    }

                    recv_ipc(
                        &mut self.threads,
                        &mut self.scheduler,
                        endpoint_ref,
                        Some(reply_ref),
                        request,
                    )
                },
            )??,
            None => {
                let endpoint_ref = self.objects.endpoint_mut(endpoint)?;
                let mut request =
                    ReceiveIpcRequest::new(endpoint, context.current(), context.cpu(), options);
                if let Some(receiver_reply) = context.reply() {
                    request = request.with_receiver_reply(receiver_reply);
                }

                recv_ipc(
                    &mut self.threads,
                    &mut self.scheduler,
                    endpoint_ref,
                    None,
                    request,
                )?
            }
        };

        match (reply, reply_cap) {
            (Some(reply), Some(reply_cap)) => {
                let descriptor = self
                    .cspace
                    .insert_reply_capability(reply, reply_cap)
                    .expect("prevalidated reply cap installation must succeed");
                Ok(ExecutionOutcome::ThreadWithReplyCap {
                    thread: action,
                    reply: descriptor,
                })
            }
            _ => Ok(ExecutionOutcome::Thread(action)),
        }
    }

    fn execute_notification_signal(
        &mut self,
        notification: ObjectId,
        badge: u64,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        self.objects
            .expect_kind(notification, KernelObjectKind::Notification)?;
        let bound_tcb = BoundTcbSignal::from_ready(
            self.objects
                .notification(notification)?
                .bound_tcb()
                .is_some_and(|bound| {
                    self.threads
                        .get(bound.thread())
                        .is_some_and(|tcb| tcb.waits_on_bound_notification_receive(notification))
                }),
        );
        let notification_ref = self.objects.notification_mut(notification)?;
        let action = signal_notification(
            &mut self.threads,
            &mut self.scheduler,
            notification_ref,
            notification,
            badge,
            bound_tcb,
        )?;
        Ok(ExecutionOutcome::Thread(action))
    }

    fn execute_notification_wait(
        &mut self,
        context: InvocationContext,
        notification: ObjectId,
        blocking: bool,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        self.objects
            .expect_kind(notification, KernelObjectKind::Notification)?;
        if !blocking {
            let notification_ref = self.objects.notification_mut(notification)?;
            let action = poll_notification(
                &self.threads,
                &self.scheduler,
                notification_ref,
                notification,
                context.current(),
                context.cpu(),
            )?;
            return Ok(ExecutionOutcome::Thread(action));
        }

        let notification_ref = self.objects.notification_mut(notification)?;
        let action = wait_notification(
            &mut self.threads,
            &mut self.scheduler,
            notification_ref,
            notification,
            context.current(),
            context.cpu(),
        )?;
        Ok(ExecutionOutcome::Thread(action))
    }

    fn execute_reply(
        &mut self,
        descriptor: CapabilityDescriptor,
        reply: ObjectId,
        caller: ObjectId,
        target: ObjectId,
        can_grant: bool,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        self.objects.expect_kind(reply, KernelObjectKind::Reply)?;
        if let ReplyState::Pending { caller: pending } = self.objects.reply(reply)?.state()
            && (pending.caller() != caller
                || pending.target() != target
                || pending.can_grant() != can_grant)
        {
            return Err(KernelExecutionError::ReplyAuthorityMismatch { reply });
        }

        let reply_ref = self.objects.reply_mut(reply)?;
        let action = reply_to_caller(&mut self.threads, &mut self.scheduler, reply_ref)?;
        self.cspace
            .consume_reply_cap(descriptor)
            .map_err(InvocationError::Cap)?;
        Ok(ExecutionOutcome::Thread(action))
    }

    fn execute_tcb_resume(
        &mut self,
        tcb: ObjectId,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        let thread = self.objects.tcb_thread(tcb)?;
        let action = resume_tcb(&mut self.threads, &mut self.scheduler, thread)?;
        Ok(ExecutionOutcome::Thread(action))
    }

    fn execute_tcb_configure(
        &mut self,
        tcb: ObjectId,
        thread: ThreadId,
        affinity: CpuId,
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        if self.threads.get(thread).is_some() {
            return Err(KernelExecutionError::ThreadAlreadyExists { thread });
        }
        self.objects.expect_kind(tcb, KernelObjectKind::Tcb)?;
        match self.objects.tcb_thread(tcb) {
            Ok(_) => {
                return Err(KernelExecutionError::Object(
                    ObjectTableError::ObjectIdAlreadyBound { object: tcb },
                ));
            }
            Err(ObjectTableError::TcbObjectUnbound { .. }) => {}
            Err(error) => return Err(KernelExecutionError::Object(error)),
        }
        if self.objects.tcb_object_for_thread(thread).is_ok() {
            return Err(KernelExecutionError::Object(
                ObjectTableError::ThreadObjectAlreadyBound { thread },
            ));
        }
        self.scheduler.run_queue(affinity)?;

        self.objects
            .bind_tcb(tcb, thread)
            .expect("prevalidated TCB binding must succeed");
        self.threads.insert(Tcb::new(thread, affinity));
        Ok(ExecutionOutcome::Thread(ThreadAction::NoThread))
    }

    fn reply_for_endpoint(
        &self,
        endpoint: ObjectId,
        reply: Option<ObjectId>,
        required: bool,
    ) -> Result<Option<ObjectId>, KernelExecutionError> {
        match (reply, required) {
            (Some(reply), _) if reply == endpoint => {
                Err(KernelExecutionError::ReplyObjectMustBeDistinct { endpoint, reply })
            }
            (Some(reply), _) => {
                self.objects.expect_kind(reply, KernelObjectKind::Reply)?;
                self.cspace
                    .validate_reply_object(reply)
                    .map_err(InvocationError::Cap)?;
                Ok(Some(reply))
            }
            (None, true) => Err(KernelExecutionError::MissingReplyObject { endpoint }),
            (None, false) => Ok(None),
        }
    }

    fn reply_from_receiver_state(
        &self,
        endpoint: ObjectId,
        receiver: ThreadId,
    ) -> Result<Option<ObjectId>, KernelExecutionError> {
        match self.threads.state(receiver) {
            Some(ThreadState::BlockedOnReceive {
                endpoint: blocked_endpoint,
                reply,
                ..
            }) if blocked_endpoint == endpoint => self.reply_for_endpoint(endpoint, reply, true),
            _ => Ok(None),
        }
    }
}

impl From<InvocationError> for KernelExecutionError {
    fn from(error: InvocationError) -> Self {
        Self::Invocation(error)
    }
}

impl From<ObjectTableError> for KernelExecutionError {
    fn from(error: ObjectTableError) -> Self {
        Self::Object(error)
    }
}

impl From<ThreadActionError> for KernelExecutionError {
    fn from(error: ThreadActionError) -> Self {
        Self::Thread(error)
    }
}

impl From<SchedulerError> for KernelExecutionError {
    fn from(error: SchedulerError) -> Self {
        Self::Scheduler(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cap::{Capability, EndpointCap, NotificationCap, ReplyCap, Rights},
        invocation::EndpointSendOp,
        ipc::Endpoint,
        notification::{Notification, NotificationState},
        reply::{Reply, ReplyCaller, ReplyCallerParams},
        scheduler::{Scheduler, SchedulerAction},
        tcb::{Tcb, ThreadState},
    };

    fn cpu(raw: u32) -> CpuId {
        CpuId::new(raw)
    }

    fn thread(raw: u64) -> ThreadId {
        ThreadId::new(raw)
    }

    fn object(raw: u64) -> ObjectId {
        ObjectId::new(raw)
    }

    fn runnable_tcb(raw: u64, affinity: CpuId) -> Tcb {
        let mut tcb = Tcb::new(thread(raw), affinity);
        tcb.set_state(ThreadState::Running);
        tcb
    }

    fn state_with_current_thread() -> (KernelState, CapabilityDescriptor, ObjectId) {
        let mut cspace = CapabilitySpace::new();
        let endpoint_descriptor = cspace
            .insert_initial_capability(Capability::Endpoint(EndpointCap {
                badge: 7,
                rights: Rights::READ | Rights::WRITE | Rights::GRANT | Rights::GRANT_REPLY,
            }))
            .unwrap();
        let endpoint_object = cspace.object_of(endpoint_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects
            .insert_endpoint(endpoint_object, Endpoint::new())
            .unwrap();
        let mut threads = ThreadTable::new();
        let tcb = runnable_tcb(1, cpu(0));
        threads.insert(tcb.clone());
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&tcb).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();

        (
            KernelState::from_parts(cspace, objects, threads, scheduler),
            endpoint_descriptor,
            endpoint_object,
        )
    }

    // KernelState tests protect host integration semantics across CSpace,
    // ObjectTable, ThreadTable, and Scheduler. Retype transaction cases live in
    // `kernel/tests/executor_retype.rs`; these tests focus on IPC, reply, TCB,
    // and executor failure-before-side-effect behavior.

    #[test]
    fn endpoint_send_invocation_blocks_current_thread() {
        let (mut state, endpoint_descriptor, endpoint_object) = state_with_current_thread();

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                endpoint_descriptor,
                Invocation::EndpointSend {
                    message_words: 0,
                    op: EndpointSendOp::Send,
                },
            ),
            Ok(ExecutionOutcome::Thread(ThreadAction::Blocked {
                thread: thread(1),
                cpu: cpu(0),
            }))
        );
        assert_eq!(
            state.threads().state(thread(1)),
            Some(ThreadState::BlockedOnSend {
                endpoint: endpoint_object,
                badge: 7,
                can_grant: true,
                can_grant_reply: true,
                is_call: false,
            })
        );
    }

    #[test]
    fn missing_endpoint_object_fails_before_thread_mutation() {
        let mut cspace = CapabilitySpace::new();
        let endpoint_descriptor = cspace
            .insert_initial_capability(Capability::Endpoint(EndpointCap {
                badge: 7,
                rights: Rights::WRITE,
            }))
            .unwrap();
        let mut threads = ThreadTable::new();
        let tcb = runnable_tcb(1, cpu(0));
        threads.insert(tcb.clone());
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&tcb).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();
        let mut state = KernelState::from_parts(cspace, ObjectTable::new(), threads, scheduler);

        assert!(matches!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                endpoint_descriptor,
                Invocation::EndpointSend {
                    message_words: 0,
                    op: EndpointSendOp::Send,
                },
            ),
            Err(KernelExecutionError::Object(
                ObjectTableError::ObjectNotFound { .. }
            ))
        ));
        assert_eq!(state.threads().state(thread(1)), Some(ThreadState::Running));
    }

    #[test]
    fn notification_signal_invocation_accumulates_badge() {
        let mut cspace = CapabilitySpace::new();
        let notification_descriptor = cspace
            .insert_initial_capability(Capability::Notification(NotificationCap {
                badge: 0b100,
                rights: Rights::WRITE,
            }))
            .unwrap();
        let notification_object = cspace.object_of(notification_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects
            .insert_notification(notification_object, Notification::new())
            .unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                notification_descriptor,
                Invocation::NotificationSignal,
            ),
            Ok(ExecutionOutcome::Thread(ThreadAction::NoThread))
        );
        assert_eq!(
            state
                .objects()
                .notification(notification_object)
                .unwrap()
                .state(),
            NotificationState::Active
        );
        assert_eq!(
            state
                .objects()
                .notification(notification_object)
                .unwrap()
                .badge(),
            0b100
        );
    }

    #[test]
    fn nonblocking_notification_wait_requires_current_thread_before_consuming_badge() {
        let mut cspace = CapabilitySpace::new();
        let notification_descriptor = cspace
            .insert_initial_capability(Capability::Notification(NotificationCap {
                badge: 0,
                rights: Rights::READ,
            }))
            .unwrap();
        let notification_object = cspace.object_of(notification_descriptor).unwrap();
        let mut notification = Notification::new();
        notification.signal(0b100, BoundTcbSignal::NotReady);
        let mut objects = ObjectTable::new();
        objects
            .insert_notification(notification_object, notification)
            .unwrap();
        let mut threads = ThreadTable::new();
        threads.insert(runnable_tcb(1, cpu(0)));
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                notification_descriptor,
                Invocation::NotificationWait { blocking: false },
            ),
            Err(KernelExecutionError::Thread(
                ThreadActionError::ThreadNotCurrent {
                    thread: thread(1),
                    cpu: cpu(0),
                }
            ))
        );
        assert_eq!(
            state
                .objects()
                .notification(notification_object)
                .unwrap()
                .state(),
            NotificationState::Active
        );
        assert_eq!(
            state
                .objects()
                .notification(notification_object)
                .unwrap()
                .badge(),
            0b100
        );
    }

    #[test]
    fn duplicate_thread_object_is_rejected_without_rebinding_object() {
        let mut state = KernelState::new(&[cpu(0), cpu(1)]).unwrap();
        state.objects_mut().insert_tcb(object(10)).unwrap();
        state
            .insert_thread_object(object(10), Tcb::new(thread(1), cpu(0)))
            .unwrap();
        state.objects_mut().insert_tcb(object(11)).unwrap();

        assert_eq!(
            state.insert_thread_object(object(11), Tcb::new(thread(1), cpu(1))),
            Err(KernelExecutionError::ThreadAlreadyExists { thread: thread(1) })
        );
        assert_eq!(state.objects().tcb_thread(object(10)), Ok(thread(1)));
        assert_eq!(
            state.objects().tcb_thread(object(11)),
            Err(ObjectTableError::TcbObjectUnbound { object: object(11) })
        );
        assert_eq!(state.threads().affinity(thread(1)), Some(cpu(0)));
    }

    #[test]
    fn insert_thread_object_rejects_unknown_cpu_without_binding_object() {
        let mut state = KernelState::new(&[cpu(0), cpu(1)]).unwrap();
        state.objects_mut().insert_tcb(object(10)).unwrap();

        assert_eq!(
            state.insert_thread_object(object(10), Tcb::new(thread(1), cpu(9))),
            Err(KernelExecutionError::Scheduler(
                SchedulerError::UnknownCpu { cpu: cpu(9) }
            ))
        );
        assert_eq!(
            state.objects().tcb_thread(object(10)),
            Err(ObjectTableError::TcbObjectUnbound { object: object(10) })
        );
        assert_eq!(state.threads().get(thread(1)), None);
    }

    #[test]
    fn reply_invocation_wakes_pending_caller() {
        let mut cspace = CapabilitySpace::new();
        let reply_descriptor = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller: object(100),
                target: object(200),
                can_grant: true,
            })
            .unwrap();
        let reply_object = cspace.object_of(reply_descriptor).unwrap();
        let mut reply = Reply::new();
        reply
            .record_caller(ReplyCaller::new(ReplyCallerParams {
                caller: object(100),
                target: object(200),
                thread: thread(1),
                cpu: cpu(0),
                can_grant: true,
            }))
            .unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_reply(reply_object, reply).unwrap();
        let mut threads = ThreadTable::new();
        let mut tcb = Tcb::new(thread(1), cpu(0));
        tcb.set_state(ThreadState::BlockedOnReply);
        threads.insert(tcb);
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(2), cpu(1)),
                reply_descriptor,
                Invocation::Reply {
                    target: object(200),
                },
            ),
            Ok(ExecutionOutcome::Thread(ThreadAction::Woken {
                thread: thread(1),
                cpu: cpu(0),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(1),
                    cpu: cpu(0),
                },
            }))
        );
        assert_eq!(state.threads().state(thread(1)), Some(ThreadState::Running));
        assert!(!state.objects().reply(reply_object).unwrap().is_pending());
        assert_eq!(
            state.cspace().lookup(reply_descriptor),
            Err(crate::cap::CapError::SlotNotFound(reply_descriptor.slot))
        );
    }

    #[test]
    fn reply_invocation_rejects_cap_metadata_mismatch_without_mutation() {
        let mut cspace = CapabilitySpace::new();
        let reply_descriptor = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller: object(100),
                target: object(201),
                can_grant: true,
            })
            .unwrap();
        let reply_object = cspace.object_of(reply_descriptor).unwrap();
        let mut reply = Reply::new();
        reply
            .record_caller(ReplyCaller::new(ReplyCallerParams {
                caller: object(100),
                target: object(200),
                thread: thread(1),
                cpu: cpu(0),
                can_grant: true,
            }))
            .unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_reply(reply_object, reply).unwrap();
        let mut threads = ThreadTable::new();
        let mut tcb = Tcb::new(thread(1), cpu(0));
        tcb.set_state(ThreadState::BlockedOnReply);
        threads.insert(tcb);
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(2), cpu(1)),
                reply_descriptor,
                Invocation::Reply {
                    target: object(201),
                },
            ),
            Err(KernelExecutionError::ReplyAuthorityMismatch {
                reply: reply_object,
            })
        );
        assert_eq!(
            state.threads().state(thread(1)),
            Some(ThreadState::BlockedOnReply)
        );
        assert!(state.objects().reply(reply_object).unwrap().is_pending());
        assert!(state.cspace().lookup(reply_descriptor).is_ok());
    }

    #[test]
    fn endpoint_call_without_reply_authority_stops_caller_without_reply_object() {
        let mut cspace = CapabilitySpace::new();
        let endpoint_descriptor = cspace
            .insert_initial_capability(Capability::Endpoint(EndpointCap {
                badge: 7,
                rights: Rights::READ | Rights::WRITE,
            }))
            .unwrap();
        let endpoint_object = cspace.object_of(endpoint_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects
            .insert_endpoint(endpoint_object, Endpoint::new())
            .unwrap();
        let mut threads = ThreadTable::new();
        let caller = runnable_tcb(1, cpu(0));
        let receiver = runnable_tcb(2, cpu(1));
        threads.insert(caller.clone());
        threads.insert(receiver.clone());
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&caller).unwrap();
        scheduler.enqueue(&receiver).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();
        scheduler.schedule_next(cpu(1)).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        state
            .execute_invocation(
                InvocationContext::new(thread(2), cpu(1)),
                endpoint_descriptor,
                Invocation::EndpointRecv { blocking: true },
            )
            .unwrap();

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                endpoint_descriptor,
                Invocation::EndpointSend {
                    message_words: 0,
                    op: EndpointSendOp::Call,
                },
            ),
            Ok(ExecutionOutcome::Thread(ThreadAction::Woken {
                thread: thread(2),
                cpu: cpu(1),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(2),
                    cpu: cpu(1),
                },
            }))
        );
        assert_eq!(
            state.threads().state(thread(1)),
            Some(ThreadState::Inactive)
        );
        assert_eq!(state.threads().state(thread(2)), Some(ThreadState::Running));
    }

    #[test]
    fn endpoint_call_records_true_caller_tcb_object() {
        let mut cspace = CapabilitySpace::new();
        let endpoint_descriptor = cspace
            .insert_initial_capability(Capability::Endpoint(EndpointCap {
                badge: 7,
                rights: Rights::READ | Rights::WRITE | Rights::GRANT | Rights::GRANT_REPLY,
            }))
            .unwrap();
        let endpoint_object = cspace.object_of(endpoint_descriptor).unwrap();

        let reply_descriptor = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller: object(1000),
                target: object(1001),
                can_grant: false,
            })
            .unwrap();
        let reply_object = cspace.object_of(reply_descriptor).unwrap();
        cspace.consume_reply_cap(reply_descriptor).unwrap();
        let caller_tcb_object = object(900);
        let receiver_tcb_object = object(901);
        let mut objects = ObjectTable::new();
        objects
            .insert_endpoint(endpoint_object, Endpoint::new())
            .unwrap();
        objects.insert_reply(reply_object, Reply::new()).unwrap();
        let mut threads = ThreadTable::new();
        let caller = runnable_tcb(1, cpu(0));
        let receiver = runnable_tcb(2, cpu(1));
        threads.insert(caller.clone());
        threads.insert(receiver.clone());
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&caller).unwrap();
        scheduler.enqueue(&receiver).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();
        scheduler.schedule_next(cpu(1)).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);
        state.objects_mut().insert_tcb(caller_tcb_object).unwrap();
        state.objects_mut().insert_tcb(receiver_tcb_object).unwrap();
        state
            .objects_mut()
            .bind_tcb(caller_tcb_object, thread(1))
            .unwrap();
        state
            .objects_mut()
            .bind_tcb(receiver_tcb_object, thread(2))
            .unwrap();
        state
            .execute_invocation(
                InvocationContext::new(thread(2), cpu(1)).with_reply(reply_object),
                endpoint_descriptor,
                Invocation::EndpointRecv { blocking: true },
            )
            .unwrap();

        let outcome = state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                endpoint_descriptor,
                Invocation::EndpointSend {
                    message_words: 0,
                    op: EndpointSendOp::Call,
                },
            )
            .unwrap();
        let ExecutionOutcome::ThreadWithReplyCap {
            thread: action,
            reply: reply_descriptor,
        } = outcome
        else {
            panic!("endpoint call must create a reply cap");
        };
        assert_eq!(
            action,
            ThreadAction::Woken {
                thread: thread(2),
                cpu: cpu(1),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(2),
                    cpu: cpu(1),
                },
            }
        );
        assert_eq!(
            state.cspace().lookup(reply_descriptor).unwrap().capability,
            Capability::Reply(ReplyCap {
                caller: caller_tcb_object,
                target: endpoint_object,
                can_grant: true,
            })
        );
        assert_eq!(
            state.cspace().object_of(reply_descriptor).unwrap(),
            reply_object
        );
        assert_eq!(
            state.objects().reply(reply_object).unwrap().state(),
            ReplyState::Pending {
                caller: ReplyCaller::new(ReplyCallerParams {
                    caller: caller_tcb_object,
                    target: endpoint_object,
                    thread: thread(1),
                    cpu: cpu(0),
                    can_grant: true,
                }),
            }
        );
    }

    #[test]
    fn immediate_call_reply_grant_comes_from_receiver_cap() {
        let mut cspace = CapabilitySpace::new();
        let endpoint_root = cspace
            .insert_initial_capability(Capability::Endpoint(EndpointCap {
                badge: 7,
                rights: Rights::READ | Rights::WRITE | Rights::GRANT | Rights::GRANT_REPLY,
            }))
            .unwrap();
        let endpoint_object = cspace.object_of(endpoint_root).unwrap();
        let caller_descriptor = cspace
            .copy(endpoint_root, Rights::WRITE | Rights::GRANT_REPLY)
            .unwrap();
        let receiver_descriptor = cspace.copy(endpoint_root, Rights::READ).unwrap();
        let reply_seed = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller: object(1000),
                target: object(1001),
                can_grant: true,
            })
            .unwrap();
        let reply_object = cspace.object_of(reply_seed).unwrap();
        cspace.consume_reply_cap(reply_seed).unwrap();
        let caller_tcb_object = object(900);
        let receiver_tcb_object = object(901);
        let mut objects = ObjectTable::new();
        objects
            .insert_endpoint(endpoint_object, Endpoint::new())
            .unwrap();
        objects.insert_reply(reply_object, Reply::new()).unwrap();
        let mut threads = ThreadTable::new();
        let caller = runnable_tcb(1, cpu(0));
        let receiver = runnable_tcb(2, cpu(1));
        threads.insert(caller.clone());
        threads.insert(receiver.clone());
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&caller).unwrap();
        scheduler.enqueue(&receiver).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();
        scheduler.schedule_next(cpu(1)).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);
        state.objects_mut().insert_tcb(caller_tcb_object).unwrap();
        state.objects_mut().insert_tcb(receiver_tcb_object).unwrap();
        state
            .objects_mut()
            .bind_tcb(caller_tcb_object, thread(1))
            .unwrap();
        state
            .objects_mut()
            .bind_tcb(receiver_tcb_object, thread(2))
            .unwrap();
        state
            .execute_invocation(
                InvocationContext::new(thread(2), cpu(1)).with_reply(reply_object),
                receiver_descriptor,
                Invocation::EndpointRecv { blocking: true },
            )
            .unwrap();

        let outcome = state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                caller_descriptor,
                Invocation::EndpointSend {
                    message_words: 0,
                    op: EndpointSendOp::Call,
                },
            )
            .unwrap();
        let ExecutionOutcome::ThreadWithReplyCap {
            reply: reply_descriptor,
            ..
        } = outcome
        else {
            panic!("endpoint call must create a reply cap");
        };

        assert_eq!(
            state.cspace().lookup(reply_descriptor).unwrap().capability,
            Capability::Reply(ReplyCap {
                caller: caller_tcb_object,
                target: endpoint_object,
                can_grant: false,
            })
        );
        assert_eq!(
            state.objects().reply(reply_object).unwrap().state(),
            ReplyState::Pending {
                caller: ReplyCaller::new(ReplyCallerParams {
                    caller: caller_tcb_object,
                    target: endpoint_object,
                    thread: thread(1),
                    cpu: cpu(0),
                    can_grant: false,
                }),
            }
        );
    }

    #[test]
    fn blocking_receive_rejects_reply_object_missing_from_cspace_without_enqueueing() {
        let mut cspace = CapabilitySpace::new();
        let endpoint_descriptor = cspace
            .insert_initial_capability(Capability::Endpoint(EndpointCap {
                badge: 7,
                rights: Rights::READ,
            }))
            .unwrap();
        let endpoint_object = cspace.object_of(endpoint_descriptor).unwrap();
        let reply_object = object(300);
        let mut objects = ObjectTable::new();
        objects
            .insert_endpoint(endpoint_object, Endpoint::new())
            .unwrap();
        objects.insert_reply(reply_object, Reply::new()).unwrap();
        let mut threads = ThreadTable::new();
        let receiver = runnable_tcb(2, cpu(1));
        threads.insert(receiver.clone());
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&receiver).unwrap();
        scheduler.schedule_next(cpu(1)).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(2), cpu(1)).with_reply(reply_object),
                endpoint_descriptor,
                Invocation::EndpointRecv { blocking: true },
            ),
            Err(KernelExecutionError::Invocation(InvocationError::Cap(
                crate::cap::CapError::ObjectNotFound(reply_object),
            )))
        );
        assert_eq!(state.threads().state(thread(2)), Some(ThreadState::Running));
        assert_eq!(
            state
                .objects()
                .endpoint(endpoint_object)
                .unwrap()
                .queued_receivers(),
            0
        );
        assert_eq!(
            state.scheduler().placement(thread(2)),
            Some(crate::scheduler::ThreadPlacement::Current { cpu: cpu(1) })
        );
    }

    #[test]
    fn queued_call_receive_creates_reply_cap_and_reply_consumes_it() {
        let mut cspace = CapabilitySpace::new();
        let endpoint_descriptor = cspace
            .insert_initial_capability(Capability::Endpoint(EndpointCap {
                badge: 7,
                rights: Rights::READ | Rights::WRITE | Rights::GRANT | Rights::GRANT_REPLY,
            }))
            .unwrap();
        let endpoint_object = cspace.object_of(endpoint_descriptor).unwrap();
        let reply_seed = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller: object(1000),
                target: object(1001),
                can_grant: false,
            })
            .unwrap();
        let reply_object = cspace.object_of(reply_seed).unwrap();
        cspace.consume_reply_cap(reply_seed).unwrap();
        let caller_tcb_object = object(900);
        let receiver_tcb_object = object(901);
        let mut objects = ObjectTable::new();
        objects
            .insert_endpoint(endpoint_object, Endpoint::new())
            .unwrap();
        objects.insert_reply(reply_object, Reply::new()).unwrap();
        let mut threads = ThreadTable::new();
        let caller = runnable_tcb(1, cpu(0));
        let receiver = runnable_tcb(2, cpu(1));
        threads.insert(caller.clone());
        threads.insert(receiver.clone());
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&caller).unwrap();
        scheduler.enqueue(&receiver).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();
        scheduler.schedule_next(cpu(1)).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);
        state.objects_mut().insert_tcb(caller_tcb_object).unwrap();
        state.objects_mut().insert_tcb(receiver_tcb_object).unwrap();
        state
            .objects_mut()
            .bind_tcb(caller_tcb_object, thread(1))
            .unwrap();
        state
            .objects_mut()
            .bind_tcb(receiver_tcb_object, thread(2))
            .unwrap();

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                endpoint_descriptor,
                Invocation::EndpointSend {
                    message_words: 0,
                    op: EndpointSendOp::Call,
                },
            ),
            Ok(ExecutionOutcome::Thread(ThreadAction::Blocked {
                thread: thread(1),
                cpu: cpu(0),
            }))
        );

        let outcome = state
            .execute_invocation(
                InvocationContext::new(thread(2), cpu(1)).with_reply(reply_object),
                endpoint_descriptor,
                Invocation::EndpointRecv { blocking: true },
            )
            .unwrap();
        let ExecutionOutcome::ThreadWithReplyCap {
            thread: action,
            reply: reply_descriptor,
        } = outcome
        else {
            panic!("receive-side call must create a reply cap");
        };
        assert_eq!(
            action,
            ThreadAction::ReplyRecorded {
                setup: crate::ipc::ReplySetup {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    can_grant: true,
                },
            }
        );
        assert_eq!(
            state.cspace().lookup(reply_descriptor).unwrap().capability,
            Capability::Reply(ReplyCap {
                caller: caller_tcb_object,
                target: endpoint_object,
                can_grant: true,
            })
        );

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(2), cpu(1)),
                reply_descriptor,
                Invocation::Reply {
                    target: endpoint_object,
                },
            ),
            Ok(ExecutionOutcome::Thread(ThreadAction::Woken {
                thread: thread(1),
                cpu: cpu(0),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(1),
                    cpu: cpu(0),
                },
            }))
        );
        assert_eq!(state.threads().state(thread(1)), Some(ThreadState::Running));
        assert!(!state.objects().reply(reply_object).unwrap().is_pending());
        assert_eq!(
            state.cspace().lookup(reply_descriptor),
            Err(crate::cap::CapError::SlotNotFound(reply_descriptor.slot))
        );
    }

    #[test]
    fn cnode_delete_final_reply_cap_removes_reply_runtime_object() {
        let mut cspace = CapabilitySpace::new();
        let cnode_descriptor = cspace
            .insert_initial_capability(Capability::CNode(crate::cap::CNodeCap::new(4)))
            .unwrap();
        let reply_descriptor = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller: object(1000),
                target: object(1001),
                can_grant: true,
            })
            .unwrap();
        let reply_object = cspace.object_of(reply_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_reply(reply_object, Reply::new()).unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.objects().get(reply_object),
            Ok(KernelObjectRef::Reply)
        );
        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode_descriptor,
                Invocation::CNodeDelete {
                    target: reply_descriptor,
                },
            ),
            Ok(ExecutionOutcome::CapabilityMutation)
        );

        assert_eq!(
            state.cspace().lookup(reply_descriptor),
            Err(crate::cap::CapError::SlotNotFound(reply_descriptor.slot))
        );
        assert_eq!(
            state.objects().get(reply_object),
            Err(ObjectTableError::ObjectNotFound {
                object: reply_object,
            })
        );
    }

    #[test]
    fn tcb_configure_binds_unbound_tcb_object_and_creates_inactive_thread() {
        let mut cspace = CapabilitySpace::new();
        let tcb_descriptor = cspace
            .insert_initial_capability(Capability::Tcb(crate::cap::TcbCap {
                rights: Rights::MANAGE,
            }))
            .unwrap();
        let tcb_object = cspace.object_of(tcb_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_tcb(tcb_object).unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                tcb_descriptor,
                Invocation::TcbConfigure {
                    thread: thread(2),
                    affinity: cpu(1),
                },
            ),
            Ok(ExecutionOutcome::Thread(ThreadAction::NoThread))
        );
        assert_eq!(state.objects().tcb_thread(tcb_object), Ok(thread(2)));
        assert_eq!(
            state.threads().state(thread(2)),
            Some(ThreadState::Inactive)
        );
        assert_eq!(state.threads().affinity(thread(2)), Some(cpu(1)));
        assert_eq!(state.scheduler().placement(thread(2)), None);
    }

    #[test]
    fn tcb_configure_rejects_unknown_cpu_without_binding_or_thread_insert() {
        let mut cspace = CapabilitySpace::new();
        let tcb_descriptor = cspace
            .insert_initial_capability(Capability::Tcb(crate::cap::TcbCap {
                rights: Rights::MANAGE,
            }))
            .unwrap();
        let tcb_object = cspace.object_of(tcb_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_tcb(tcb_object).unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                tcb_descriptor,
                Invocation::TcbConfigure {
                    thread: thread(2),
                    affinity: cpu(9),
                },
            ),
            Err(KernelExecutionError::Scheduler(
                SchedulerError::UnknownCpu { cpu: cpu(9) }
            ))
        );
        assert_eq!(
            state.objects().tcb_thread(tcb_object),
            Err(ObjectTableError::TcbObjectUnbound { object: tcb_object })
        );
        assert_eq!(state.threads().get(thread(2)), None);
    }

    #[test]
    fn tcb_configure_rejects_already_bound_object_without_thread_insert() {
        let mut cspace = CapabilitySpace::new();
        let tcb_descriptor = cspace
            .insert_initial_capability(Capability::Tcb(crate::cap::TcbCap {
                rights: Rights::MANAGE,
            }))
            .unwrap();
        let tcb_object = cspace.object_of(tcb_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_tcb(tcb_object).unwrap();
        objects.bind_tcb(tcb_object, thread(2)).unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                tcb_descriptor,
                Invocation::TcbConfigure {
                    thread: thread(3),
                    affinity: cpu(1),
                },
            ),
            Err(KernelExecutionError::Object(
                ObjectTableError::ObjectIdAlreadyBound { object: tcb_object }
            ))
        );
        assert_eq!(state.objects().tcb_thread(tcb_object), Ok(thread(2)));
        assert_eq!(state.threads().get(thread(3)), None);
    }

    #[test]
    fn tcb_configure_rejects_existing_thread_without_rebinding_object() {
        let mut cspace = CapabilitySpace::new();
        let tcb_descriptor = cspace
            .insert_initial_capability(Capability::Tcb(crate::cap::TcbCap {
                rights: Rights::MANAGE,
            }))
            .unwrap();
        let tcb_object = cspace.object_of(tcb_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_tcb(tcb_object).unwrap();
        let mut threads = ThreadTable::new();
        threads.insert(Tcb::new(thread(2), cpu(1)));
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                tcb_descriptor,
                Invocation::TcbConfigure {
                    thread: thread(2),
                    affinity: cpu(1),
                },
            ),
            Err(KernelExecutionError::ThreadAlreadyExists { thread: thread(2) })
        );
        assert_eq!(
            state.objects().tcb_thread(tcb_object),
            Err(ObjectTableError::TcbObjectUnbound { object: tcb_object })
        );
        assert_eq!(state.threads().affinity(thread(2)), Some(cpu(1)));
    }

    #[test]
    fn tcb_resume_invocation_restarts_bound_thread() {
        let mut cspace = CapabilitySpace::new();
        let tcb_descriptor = cspace
            .insert_initial_capability(Capability::Tcb(crate::cap::TcbCap {
                rights: Rights::MANAGE,
            }))
            .unwrap();
        let tcb_object = cspace.object_of(tcb_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_tcb(tcb_object).unwrap();
        objects.bind_tcb(tcb_object, thread(2)).unwrap();
        let mut threads = ThreadTable::new();
        threads.insert(Tcb::new(thread(2), cpu(1)));
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                tcb_descriptor,
                Invocation::TcbResume,
            ),
            Ok(ExecutionOutcome::Thread(ThreadAction::Resumed {
                thread: thread(2),
                cpu: cpu(1),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(2),
                    cpu: cpu(1),
                },
            }))
        );
        assert_eq!(state.threads().state(thread(2)), Some(ThreadState::Restart));
        assert_eq!(
            state.scheduler().placement(thread(2)),
            Some(crate::scheduler::ThreadPlacement::Ready { cpu: cpu(1) })
        );
    }

    #[test]
    fn tcb_resume_rejects_unbound_tcb_object_without_thread_mutation() {
        let mut cspace = CapabilitySpace::new();
        let tcb_descriptor = cspace
            .insert_initial_capability(Capability::Tcb(crate::cap::TcbCap {
                rights: Rights::MANAGE,
            }))
            .unwrap();
        let tcb_object = cspace.object_of(tcb_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_tcb(tcb_object).unwrap();
        let mut threads = ThreadTable::new();
        threads.insert(Tcb::new(thread(2), cpu(1)));
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                tcb_descriptor,
                Invocation::TcbResume,
            ),
            Err(KernelExecutionError::Object(
                ObjectTableError::TcbObjectUnbound { object: tcb_object }
            ))
        );
        assert_eq!(
            state.threads().state(thread(2)),
            Some(ThreadState::Inactive)
        );
        assert_eq!(state.scheduler().placement(thread(2)), None);
    }

    #[test]
    fn tcb_resume_rejects_bound_missing_thread_without_scheduler_mutation() {
        let mut cspace = CapabilitySpace::new();
        let tcb_descriptor = cspace
            .insert_initial_capability(Capability::Tcb(crate::cap::TcbCap {
                rights: Rights::MANAGE,
            }))
            .unwrap();
        let tcb_object = cspace.object_of(tcb_descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_tcb(tcb_object).unwrap();
        objects.bind_tcb(tcb_object, thread(2)).unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                tcb_descriptor,
                Invocation::TcbResume,
            ),
            Err(KernelExecutionError::Thread(
                ThreadActionError::UnknownThread { thread: thread(2) }
            ))
        );
        assert_eq!(state.scheduler().run_queue(cpu(0)).unwrap().ready_len(), 0);
        assert_eq!(state.scheduler().run_queue(cpu(1)).unwrap().ready_len(), 0);
    }
}
