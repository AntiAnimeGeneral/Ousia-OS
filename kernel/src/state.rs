use crate::{
    cap::{CapabilityDescriptor, CapabilitySpace, ObjectId, ReplyCap, RetypeTarget},
    invocation::{Invocation, InvocationError, InvocationOutcome, invoke},
    ipc::{Endpoint, IpcPayload, IpcReceiveOptions, IpcSendOptions},
    notification::Notification,
    object::{KernelObjectKind, ObjectTable, ObjectTableError},
    reply::ReplyState,
    scheduler::{Scheduler, SchedulerError},
    tcb::{CpuId, Tcb, ThreadId, ThreadState},
    thread_action::{
        ThreadAction, ThreadActionError, ThreadTable, poll_notification, recv_ipc, reply_to_caller,
        resume_tcb, send_ipc, signal_notification, wait_notification,
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
    Retyped {
        descriptor: CapabilityDescriptor,
    },
    Unsupported(UnsupportedInvocation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedInvocation {
    FrameMap,
    UntypedRetype,
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
            } => self.execute_endpoint_send(
                context,
                endpoint,
                badge,
                IpcSendOptions {
                    blocking,
                    is_call,
                    can_grant,
                    can_grant_reply,
                },
            ),
            InvocationOutcome::ReceiveIpcAuthorized {
                endpoint,
                blocking,
                can_grant,
            } => self.execute_endpoint_recv(
                context,
                endpoint,
                IpcReceiveOptions {
                    blocking,
                    can_grant,
                },
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
            InvocationOutcome::UntypedRetypeAuthorized { target, .. } => {
                self.execute_untyped_retype(descriptor, target)
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
    ) -> Result<ExecutionOutcome, KernelExecutionError> {
        let object = self
            .cspace
            .preview_retype_untyped(source, &target)
            .map_err(InvocationError::Cap)?;

        match &target {
            RetypeTarget::Endpoint | RetypeTarget::Notification | RetypeTarget::Tcb { .. } => {
                self.objects.validate_unbound(object)?;
            }
            RetypeTarget::Frame { .. } | RetypeTarget::CNode { .. } => {
                return Ok(ExecutionOutcome::Unsupported(
                    UnsupportedInvocation::UntypedRetype,
                ));
            }
            RetypeTarget::Untyped { .. } => {}
        }

        let descriptor = self
            .cspace
            .retype_untyped(source, target.clone())
            .expect("prevalidated untyped retype must succeed");
        match target {
            RetypeTarget::Endpoint => self
                .objects
                .insert_endpoint(object, Endpoint::new())
                .expect("prevalidated endpoint object insertion must succeed"),
            RetypeTarget::Notification => self
                .objects
                .insert_notification(object, Notification::new())
                .expect("prevalidated notification object insertion must succeed"),
            RetypeTarget::Tcb { .. } => self
                .objects
                .insert_tcb(object)
                .expect("prevalidated TCB object insertion must succeed"),
            RetypeTarget::Untyped { .. } => {}
            RetypeTarget::Frame { .. } | RetypeTarget::CNode { .. } => {
                unreachable!("unsupported retype target returned before commit")
            }
        }

        Ok(ExecutionOutcome::Retyped { descriptor })
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
        let reply = if options.is_call {
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
            Some(reply) => {
                let (endpoint_ref, reply_ref) =
                    self.objects.endpoint_and_reply_mut(endpoint, reply)?;
                send_ipc(
                    &mut self.threads,
                    &mut self.scheduler,
                    endpoint_ref,
                    Some(reply_ref),
                    endpoint,
                    caller_object,
                    context.current(),
                    context.cpu(),
                    badge,
                    options,
                    context.payload(),
                )?
            }
            None => {
                let endpoint_ref = self.objects.endpoint_mut(endpoint)?;
                send_ipc(
                    &mut self.threads,
                    &mut self.scheduler,
                    endpoint_ref,
                    None,
                    endpoint,
                    caller_object,
                    context.current(),
                    context.cpu(),
                    badge,
                    options,
                    context.payload(),
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
        let queued_call_requires_reply = self
            .objects
            .endpoint(endpoint)?
            .next_sender()
            .is_some_and(|message| message.is_call());
        let reply =
            self.reply_for_endpoint(endpoint, context.reply(), queued_call_requires_reply)?;
        let caller_object = if queued_call_requires_reply {
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
            Some(reply) => {
                let (endpoint_ref, reply_ref) =
                    self.objects.endpoint_and_reply_mut(endpoint, reply)?;
                recv_ipc(
                    &mut self.threads,
                    &mut self.scheduler,
                    endpoint_ref,
                    Some(reply_ref),
                    endpoint,
                    caller_object,
                    context.reply(),
                    context.current(),
                    context.cpu(),
                    options,
                )?
            }
            None => {
                let endpoint_ref = self.objects.endpoint_mut(endpoint)?;
                recv_ipc(
                    &mut self.threads,
                    &mut self.scheduler,
                    endpoint_ref,
                    None,
                    endpoint,
                    caller_object,
                    context.reply(),
                    context.current(),
                    context.cpu(),
                    options,
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
        let bound_tcb_accepts_receive = self
            .objects
            .notification(notification)?
            .bound_tcb()
            .is_some_and(|bound| {
                self.threads
                    .get(bound.thread())
                    .is_some_and(|tcb| tcb.waits_on_bound_notification_receive(notification))
            });
        let notification_ref = self.objects.notification_mut(notification)?;
        let action = signal_notification(
            &mut self.threads,
            &mut self.scheduler,
            notification_ref,
            notification,
            badge,
            bound_tcb_accepts_receive,
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
        ipc::Endpoint,
        notification::{Notification, NotificationState},
        reply::{Reply, ReplyCaller},
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

    #[test]
    fn endpoint_send_invocation_blocks_current_thread() {
        let (mut state, endpoint_descriptor, endpoint_object) = state_with_current_thread();

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                endpoint_descriptor,
                Invocation::EndpointSend {
                    message_words: 0,
                    blocking: true,
                    is_call: false,
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
                    blocking: true,
                    is_call: false,
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
        notification.signal(0b100, false);
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
            .record_caller(ReplyCaller::new(
                object(100),
                object(200),
                thread(1),
                cpu(0),
                true,
            ))
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
            .record_caller(ReplyCaller::new(
                object(100),
                object(200),
                thread(1),
                cpu(0),
                true,
            ))
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
                    blocking: true,
                    is_call: true,
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
                caller: ReplyCaller::new(
                    caller_tcb_object,
                    endpoint_object,
                    thread(1),
                    cpu(0),
                    true
                ),
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
                    blocking: true,
                    is_call: true,
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
                caller: ReplyCaller::new(
                    caller_tcb_object,
                    endpoint_object,
                    thread(1),
                    cpu(0),
                    false
                ),
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
                    blocking: true,
                    is_call: true,
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
    fn untyped_retype_endpoint_creates_object_and_capability() {
        let mut cspace = CapabilitySpace::new();
        let untyped = cspace
            .insert_initial_capability(Capability::Untyped(crate::cap::UntypedCap {
                size_bits: 12,
            }))
            .unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, ObjectTable::new(), threads, scheduler);

        let outcome = state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: crate::cap::RetypeTarget::Endpoint,
                },
            )
            .unwrap();
        let ExecutionOutcome::Retyped { descriptor } = outcome else {
            panic!("untyped endpoint retype must return a new capability descriptor");
        };
        let endpoint_object = state.cspace().object_of(descriptor).unwrap();

        assert_eq!(
            state.cspace().lookup(descriptor).unwrap().capability,
            Capability::Endpoint(EndpointCap {
                badge: 0,
                rights: Rights::READ | Rights::WRITE | Rights::GRANT | Rights::GRANT_REPLY,
            })
        );
        assert_eq!(
            state
                .objects()
                .expect_kind(endpoint_object, KernelObjectKind::Endpoint),
            Ok(crate::object::KernelObjectRef::Endpoint)
        );
    }

    #[test]
    fn untyped_retype_notification_creates_object_and_can_signal() {
        let mut cspace = CapabilitySpace::new();
        let untyped = cspace
            .insert_initial_capability(Capability::Untyped(crate::cap::UntypedCap {
                size_bits: 12,
            }))
            .unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, ObjectTable::new(), threads, scheduler);

        let outcome = state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: crate::cap::RetypeTarget::Notification,
                },
            )
            .unwrap();
        let ExecutionOutcome::Retyped { descriptor } = outcome else {
            panic!("untyped notification retype must return a new capability descriptor");
        };
        let notification_object = state.cspace().object_of(descriptor).unwrap();

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                descriptor,
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
    }

    #[test]
    fn unsupported_untyped_retype_target_does_not_commit_cspace_or_objects() {
        let mut cspace = CapabilitySpace::new();
        let untyped = cspace
            .insert_initial_capability(Capability::Untyped(crate::cap::UntypedCap {
                size_bits: 12,
            }))
            .unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, ObjectTable::new(), threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: crate::cap::RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                },
            ),
            Ok(ExecutionOutcome::Unsupported(
                UnsupportedInvocation::UntypedRetype
            ))
        );

        let endpoint = state
            .cspace_mut()
            .retype_untyped(untyped, crate::cap::RetypeTarget::Endpoint)
            .unwrap();
        assert_eq!(endpoint.slot.raw(), untyped.slot.raw() + 1);
        assert_eq!(
            state
                .objects()
                .get(state.cspace().object_of(endpoint).unwrap()),
            Err(ObjectTableError::ObjectNotFound {
                object: state.cspace().object_of(endpoint).unwrap()
            })
        );
    }

    #[test]
    fn untyped_retype_object_table_conflict_does_not_commit_cspace() {
        let mut cspace = CapabilitySpace::new();
        let untyped = cspace
            .insert_initial_capability(Capability::Untyped(crate::cap::UntypedCap {
                size_bits: 12,
            }))
            .unwrap();
        let predicted_object = cspace
            .preview_retype_untyped(untyped, &crate::cap::RetypeTarget::Endpoint)
            .unwrap();
        let mut objects = ObjectTable::new();
        objects
            .insert_endpoint(predicted_object, Endpoint::new())
            .unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: crate::cap::RetypeTarget::Endpoint,
                },
            ),
            Err(KernelExecutionError::Object(
                ObjectTableError::ObjectIdAlreadyBound {
                    object: predicted_object,
                }
            ))
        );

        let endpoint = state
            .cspace_mut()
            .retype_untyped(untyped, crate::cap::RetypeTarget::Endpoint)
            .unwrap();
        assert_eq!(endpoint.slot.raw(), untyped.slot.raw() + 1);
        assert_eq!(state.cspace().object_of(endpoint), Ok(predicted_object));
    }

    #[test]
    fn untyped_retype_tcb_object_table_conflict_does_not_commit_cspace() {
        let mut cspace = CapabilitySpace::new();
        let untyped = cspace
            .insert_initial_capability(Capability::Untyped(crate::cap::UntypedCap {
                size_bits: 12,
            }))
            .unwrap();
        let target = crate::cap::RetypeTarget::Tcb {
            rights: Rights::MANAGE,
        };
        let predicted_object = cspace.preview_retype_untyped(untyped, &target).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_tcb(predicted_object).unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, objects, threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype { target },
            ),
            Err(KernelExecutionError::Object(
                ObjectTableError::ObjectIdAlreadyBound {
                    object: predicted_object,
                }
            ))
        );

        let endpoint = state
            .cspace_mut()
            .retype_untyped(untyped, crate::cap::RetypeTarget::Endpoint)
            .unwrap();
        assert_eq!(endpoint.slot.raw(), untyped.slot.raw() + 1);
        assert_eq!(state.cspace().object_of(endpoint), Ok(predicted_object));
    }

    #[test]
    fn untyped_retype_nested_untyped_commits_cspace_without_object_table_entry() {
        let mut cspace = CapabilitySpace::new();
        let untyped = cspace
            .insert_initial_capability(Capability::Untyped(crate::cap::UntypedCap {
                size_bits: 12,
            }))
            .unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, ObjectTable::new(), threads, scheduler);

        let outcome = state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: crate::cap::RetypeTarget::Untyped { size_bits: 10 },
                },
            )
            .unwrap();
        let ExecutionOutcome::Retyped { descriptor } = outcome else {
            panic!("nested untyped retype must return a new capability descriptor");
        };
        let child_object = state.cspace().object_of(descriptor).unwrap();

        assert_eq!(
            state.cspace().lookup(descriptor).unwrap().capability,
            Capability::Untyped(crate::cap::UntypedCap { size_bits: 10 })
        );
        assert_eq!(
            state.objects().get(child_object),
            Err(ObjectTableError::ObjectNotFound {
                object: child_object
            })
        );
    }

    #[test]
    fn oversized_nested_untyped_retype_does_not_commit_cspace() {
        let mut cspace = CapabilitySpace::new();
        let untyped = cspace
            .insert_initial_capability(Capability::Untyped(crate::cap::UntypedCap {
                size_bits: 12,
            }))
            .unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, ObjectTable::new(), threads, scheduler);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: crate::cap::RetypeTarget::Untyped { size_bits: 13 },
                },
            ),
            Err(KernelExecutionError::Invocation(
                InvocationError::InvalidRetypeSize {
                    requested: 13,
                    source: 12,
                }
            ))
        );

        let endpoint = state
            .cspace_mut()
            .retype_untyped(untyped, crate::cap::RetypeTarget::Endpoint)
            .unwrap();
        assert_eq!(endpoint.slot.raw(), untyped.slot.raw() + 1);
    }

    #[test]
    fn untyped_retype_tcb_creates_unbound_tcb_object() {
        let mut cspace = CapabilitySpace::new();
        let untyped = cspace
            .insert_initial_capability(Capability::Untyped(crate::cap::UntypedCap {
                size_bits: 12,
            }))
            .unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let mut state = KernelState::from_parts(cspace, ObjectTable::new(), threads, scheduler);

        let outcome = state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: crate::cap::RetypeTarget::Tcb {
                        rights: Rights::MANAGE,
                    },
                },
            )
            .unwrap();
        let ExecutionOutcome::Retyped { descriptor } = outcome else {
            panic!("TCB retype must return a new capability descriptor");
        };
        let tcb_object = state.cspace().object_of(descriptor).unwrap();

        assert_eq!(
            state.cspace().lookup(descriptor).unwrap().capability,
            Capability::Tcb(crate::cap::TcbCap {
                rights: Rights::MANAGE,
            })
        );
        assert_eq!(
            state.objects().tcb_thread(tcb_object),
            Err(ObjectTableError::TcbObjectUnbound { object: tcb_object })
        );
        assert_eq!(state.threads().get(thread(2)), None);
        assert_eq!(state.scheduler().placement(thread(2)), None);
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
