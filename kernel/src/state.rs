use crate::{
    cap::{CapabilityDescriptor, CapabilitySpace, ObjectId},
    invocation::{Invocation, InvocationError, InvocationOutcome, invoke},
    ipc::{IpcPayload, IpcReceiveOptions, IpcSendOptions},
    object::{KernelObjectKind, ObjectTable, ObjectTableError},
    reply::ReplyState,
    scheduler::{Scheduler, SchedulerError},
    tcb::{CpuId, Tcb, ThreadId},
    thread_action::{
        ThreadAction, ThreadActionError, ThreadTable, poll_notification, recv_ipc, reply_to_caller,
        send_ipc, signal_notification, wait_notification,
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
    Unsupported(UnsupportedInvocation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedInvocation {
    FrameMap,
    UntypedRetype,
    TcbResume,
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
            InvocationOutcome::UntypedRetypeAuthorized { .. } => Ok(ExecutionOutcome::Unsupported(
                UnsupportedInvocation::UntypedRetype,
            )),
            InvocationOutcome::TcbResumeAuthorized { .. } => Ok(ExecutionOutcome::Unsupported(
                UnsupportedInvocation::TcbResume,
            )),
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
        let reply = self.reply_for_endpoint(endpoint, context.reply(), options.is_call)?;
        let caller_object = if options.is_call {
            Some(self.objects.tcb_object_for_thread(context.current())?)
        } else {
            None
        };

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

        Ok(ExecutionOutcome::Thread(action))
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
                    context.current(),
                    context.cpu(),
                    options,
                )?
            }
        };

        Ok(ExecutionOutcome::Thread(action))
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
                Ok(Some(reply))
            }
            (None, true) => Err(KernelExecutionError::MissingReplyObject { endpoint }),
            (None, false) => Ok(None),
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
        state
            .insert_thread_object(object(10), Tcb::new(thread(1), cpu(0)))
            .unwrap();

        assert_eq!(
            state.insert_thread_object(object(11), Tcb::new(thread(1), cpu(1))),
            Err(KernelExecutionError::ThreadAlreadyExists { thread: thread(1) })
        );
        assert_eq!(state.objects().tcb_thread(object(10)), Ok(thread(1)));
        assert_eq!(
            state.objects().tcb_thread(object(11)),
            Err(ObjectTableError::ObjectNotFound { object: object(11) })
        );
        assert_eq!(state.threads().affinity(thread(1)), Some(cpu(0)));
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
        let reply_object = object(300);
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
                InvocationContext::new(thread(2), cpu(1)),
                endpoint_descriptor,
                Invocation::EndpointRecv { blocking: true },
            )
            .unwrap();

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)).with_reply(reply_object),
                endpoint_descriptor,
                Invocation::EndpointSend {
                    message_words: 0,
                    blocking: true,
                    is_call: true,
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
}
