use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use crate::{ConnectOptions, Error, ErrorCode, Session};
use tokio::sync::Notify;

/// GPASCII members with stable calling-thread-to-member and channel affinity.
#[derive(Debug, Clone, Copy)]
pub struct PoolProfile {
    pub target_size: u32,
    pub max_members: u32,
    pub min_ready: u32,
    pub connect_timeout_ms: u32,
}
impl Default for PoolProfile {
    fn default() -> Self {
        Self {
            target_size: 1,
            max_members: 4,
            min_ready: 1,
            connect_timeout_ms: 60_000,
        }
    }
}
impl PoolProfile {
    pub fn validate(&self) -> Result<(), Error> {
        if self.target_size == 0
            || self.target_size > self.max_members
            || self.max_members > 255
            || self.min_ready == 0
            || self.min_ready > self.target_size
            || self.connect_timeout_ms == 0
        {
            return Err(Error::invalid("pool requires 1 <= min_ready <= target <= max_members <= 255 and a connection budget"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MemberState {
    Ready = 0,
    Busy = 1,
    Failed = 2,
    Retiring = 3,
    ReleaseUncertain = 4,
    Creating = 5,
    Drained = 6,
}
#[derive(Debug, Clone, Copy)]
pub struct PoolSnapshot {
    pub revision: u64,
    pub desired: u32,
    pub ready: u32,
    pub busy: u32,
    pub creating: u32,
    pub failed: u32,
    pub retiring: u32,
    pub release_uncertain: u32,
    pub max_members: u32,
    pub unknown_write: bool,
    pub maintenance_active: bool,
    pub maintenance_failed: bool,
    pub last_create_error: ErrorCode,
}
#[derive(Debug, Clone, Copy)]
pub struct MemberSnapshot {
    pub member_id: u64,
    pub epoch: u64,
    pub state: MemberState,
    pub control: bool,
    pub last_response_unix_ms: u64,
    pub last_error: ErrorCode,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobClass {
    Ordered,
    ReadPool,
    Maintenance,
    File,
    Shell,
}
#[derive(Clone, Copy)]
struct Ticket {
    class: JobClass,
    generation: u64,
    member_id: u64,
    bound_epoch: u64,
}
pub(crate) struct Member {
    pub id: u64,
    pub epoch: u64,
    pub state: MemberState,
    pub session: Option<Arc<dyn Session>>,
    pub last_response: u64,
    pub drain: bool,
    pub last_error: ErrorCode,
}
pub(crate) struct ThreadAffinity {
    pub(crate) member_id: u64,
    owner: Weak<()>,
}
pub(crate) struct PoolState {
    pub revision: u64,
    pub generation: u64,
    pub profile: PoolProfile,
    pub members: Vec<Member>,
    pub options: Option<ConnectOptions>,
    pub uncertain: Vec<(u64, u64)>,
    pub creating: u32,
    pub running: usize,
    pub maintenance_active: bool,
    pub maintenance_failed: bool,
    pub maintenance_blocked: bool,
    pub maintenance_running: u32,
    pub unknown_write: bool,
    pub closing: bool,
    pub last_create_error: ErrorCode,
    pending: BTreeMap<u64, Ticket>,
    pub(crate) affinity: HashMap<std::thread::ThreadId, ThreadAffinity>,
    quota: Vec<crate::budget::AdmissionPermit>,
}
pub(crate) struct Pool {
    pub state: Mutex<PoolState>,
    pub changed: Notify,
}
impl Pool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(PoolState {
                revision: 0,
                generation: 0,
                profile: PoolProfile::default(),
                members: Vec::new(),
                options: None,
                uncertain: Vec::new(),
                creating: 0,
                running: 0,
                maintenance_active: false,
                maintenance_failed: false,
                maintenance_blocked: false,
                maintenance_running: 0,
                unknown_write: false,
                closing: false,
                last_create_error: ErrorCode::Ok,
                pending: BTreeMap::new(),
                affinity: HashMap::new(),
                quota: Vec::new(),
            }),
            changed: Notify::new(),
        })
    }
    pub fn lock(&self) -> MutexGuard<'_, PoolState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub fn set_profile(&self, profile: PoolProfile) -> Result<(), Error> {
        profile.validate()?;
        static CONNECTION_BUDGET: std::sync::OnceLock<Arc<crate::budget::Budget>> =
            std::sync::OnceLock::new();
        let budget = CONNECTION_BUDGET.get_or_init(|| Arc::new(crate::budget::Budget::new(256, 0)));
        let mut state = self.lock();
        let occupied = state
            .members
            .iter()
            .filter(|m| {
                matches!(
                    m.state,
                    MemberState::Ready
                        | MemberState::Busy
                        | MemberState::Retiring
                        | MemberState::Creating
                )
            })
            .count()
            + state.uncertain.len();
        if (profile.max_members as usize) < occupied {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "profile quota is below live and release-uncertain occupancy",
            ));
        }
        let extra = (state.quota.len()..profile.max_members as usize)
            .map(|_| budget.reserve(0))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                Error::new(
                    ErrorCode::ResourceLimit,
                    "process-wide reserved GPASCII capacity is full (256 members)",
                )
            })?;
        state.quota.extend(extra);
        state.quota.truncate(profile.max_members as usize);
        state.profile = profile;
        Ok(())
    }
    pub fn snapshot(&self) -> PoolSnapshot {
        let mut s = self.lock();
        observe_health(&mut s);
        PoolSnapshot {
            revision: s.revision,
            desired: s.profile.target_size,
            ready: s
                .members
                .iter()
                .filter(|m| {
                    m.state == MemberState::Ready
                        && !m.drain
                        && m.session.as_ref().is_some_and(|v| v.is_healthy())
                })
                .count() as u32,
            busy: s
                .members
                .iter()
                .filter(|m| m.state == MemberState::Busy)
                .count() as u32,
            creating: s.creating,
            failed: s
                .members
                .iter()
                .filter(|m| m.state == MemberState::Failed)
                .count() as u32,
            retiring: s
                .members
                .iter()
                .filter(|m| m.state == MemberState::Retiring)
                .count() as u32,
            release_uncertain: s.uncertain.len() as u32,
            max_members: s.profile.max_members,
            unknown_write: s.unknown_write,
            maintenance_active: s.maintenance_active,
            maintenance_failed: s.maintenance_failed,
            last_create_error: s.last_create_error,
        }
    }
    pub fn members(&self) -> Vec<MemberSnapshot> {
        let mut state = self.lock();
        observe_health(&mut state);
        state
            .members
            .iter()
            .map(|m| MemberSnapshot {
                member_id: m.id,
                epoch: m.epoch,
                state: if m.state == MemberState::Ready
                    && !m.session.as_ref().is_some_and(|v| v.is_healthy())
                {
                    MemberState::Failed
                } else {
                    m.state
                },
                control: m.id == 1,
                last_response_unix_ms: m.last_response,
                last_error: m.last_error,
            })
            .collect()
    }
    pub fn register(
        &self,
        seq: u64,
        class: JobClass,
        writes: bool,
        origin_thread: std::thread::ThreadId,
        owner: Weak<()>,
    ) -> Result<(), Error> {
        let mut s = self.lock();
        reclaim_exited_affinity(&mut s.affinity);
        observe_health(&mut s);
        if s.closing {
            return Err(Error::new(ErrorCode::Closing, "pool is closing"));
        }
        if s.unknown_write && writes {
            return Err(Error::new(
                ErrorCode::RecoveryRequired,
                "an unknown write requires explicit application recovery acknowledgement",
            ));
        }
        if s.maintenance_active || s.pending.values().any(|t| t.class == JobClass::Maintenance) {
            return Err(Error::new(
                ErrorCode::MaintenanceActive,
                "device maintenance barrier is pending or active",
            ));
        }
        if s.members.is_empty() {
            return Err(Error::new(
                ErrorCode::NoReadyLane,
                "pool has no prepared members",
            ));
        }
        let member_id = if matches!(class, JobClass::Ordered | JobClass::ReadPool) {
            if let Some(affinity) = s.affinity.get(&origin_thread) {
                affinity.member_id
            } else {
                let selected = s
                    .members
                    .iter()
                    .filter(|member| {
                        !member.drain
                            && matches!(member.state, MemberState::Ready | MemberState::Busy)
                            && member.session.as_ref().is_some_and(|v| v.is_healthy())
                    })
                    .min_by_key(|member| {
                        (
                            s.affinity
                                .values()
                                .filter(|affinity| affinity.member_id == member.id)
                                .count(),
                            member.id,
                        )
                    })
                    .or_else(|| s.members.first())
                    .ok_or_else(|| Error::new(ErrorCode::NoReadyLane, "no GPASCII member"))?
                    .id;
                s.affinity.insert(
                    origin_thread,
                    ThreadAffinity {
                        member_id: selected,
                        owner,
                    },
                );
                selected
            }
        } else {
            s.members.first().map_or(0, |member| member.id)
        };
        let bound_epoch = s
            .members
            .iter()
            .find(|member| member.id == member_id)
            .ok_or_else(|| Error::new(ErrorCode::NoReadyLane, "thread affinity member is absent"))?
            .epoch;
        let generation = s.generation;
        s.pending.insert(
            seq,
            Ticket {
                class,
                generation,
                member_id,
                bound_epoch,
            },
        );
        Ok(())
    }
    pub fn maintenance_pending(&self) -> bool {
        let state = self.lock();
        state.maintenance_active
            || state
                .pending
                .values()
                .any(|ticket| ticket.class == JobClass::Maintenance)
    }
    pub fn unregister(&self, seq: u64) {
        self.lock().pending.remove(&seq);
        self.changed.notify_waiters();
    }
    pub fn try_acquire(self: &Arc<Self>, seq: u64, writes: bool) -> Result<Option<Lease>, Error> {
        let mut s = self.lock();
        observe_health(&mut s);
        let t = *s
            .pending
            .get(&seq)
            .ok_or_else(|| Error::new(ErrorCode::Cancelled, "request is no longer queued"))?;
        if s.closing {
            return Err(Error::new(ErrorCode::Closing, "pool is closing"));
        }
        if t.generation != s.generation {
            return Err(Error::disconnected(
                "request belongs to a previous device generation",
            ));
        }
        if writes && s.unknown_write {
            return Err(Error::new(
                ErrorCode::RecoveryRequired,
                "unknown write blocks conflicting operations",
            ));
        }
        if s.maintenance_active {
            return Ok(None);
        }
        if s.pending
            .range(..seq)
            .any(|(_, p)| p.class == JobClass::Maintenance)
        {
            return Ok(None);
        }
        if t.class == JobClass::Maintenance
            && (s.running != 0 || s.creating != 0 || s.pending.range(..seq).next().is_some())
        {
            return Ok(None);
        }
        if matches!(t.class, JobClass::Ordered | JobClass::ReadPool)
            && s.pending.range(..seq).any(|(_, pending)| {
                matches!(pending.class, JobClass::Ordered | JobClass::ReadPool)
                    && pending.member_id == t.member_id
            })
        {
            return Ok(None);
        }
        let index = if matches!(
            t.class,
            JobClass::Ordered | JobClass::ReadPool | JobClass::Maintenance
        ) {
            let (index, member) = s
                .members
                .iter()
                .enumerate()
                .find(|(_, member)| member.id == t.member_id)
                .ok_or_else(|| {
                    Error::new(ErrorCode::NoReadyLane, "no thread-affined GPASCII member")
                })?;
            if member.epoch != t.bound_epoch {
                return Err(Error::disconnected(
                    "queued request is bound to a retired GPASCII epoch",
                ));
            }
            if member.state == MemberState::Busy {
                return Ok(None);
            }
            if member.drain
                || member.state != MemberState::Ready
                || !member.session.as_ref().is_some_and(|v| v.is_healthy())
            {
                return Err(Error::new(
                    ErrorCode::NoReadyLane,
                    "thread-affined GPASCII member is unavailable",
                ));
            }
            index
        } else {
            if s.maintenance_blocked {
                return Ok(None);
            }
            if s.members.first().is_some_and(|m| m.epoch != t.bound_epoch) {
                return Err(Error::disconnected(
                    "maintenance request belongs to an old session owner",
                ));
            }
            if s.maintenance_failed {
                return Err(Error::disconnected(
                    "maintenance connection requires explicit repair",
                ));
            }
            if s.pending.range(..seq).any(|(_, p)| p.class == t.class) {
                return Ok(None);
            }
            if s.members.is_empty() {
                return Err(Error::new(
                    ErrorCode::NoReadyLane,
                    "no session for maintenance",
                ));
            }
            0
        };
        let session = s.members[index]
            .session
            .as_ref()
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NoReadyLane, "member has no session"))?;
        let id = s.members[index].id;
        let epoch = s.members[index].epoch;
        let uses_control = matches!(
            t.class,
            JobClass::Ordered | JobClass::ReadPool | JobClass::Maintenance
        );
        if uses_control {
            s.members[index].state = MemberState::Busy;
        }
        s.running += 1;
        if !uses_control {
            s.maintenance_running += 1;
        }
        if t.class == JobClass::Maintenance {
            s.maintenance_active = true;
        }
        s.pending.remove(&seq);
        Ok(Some(Lease {
            pool: self.clone(),
            member_id: id,
            epoch,
            session,
            class: t.class,
            uses_control,
        }))
    }
}
pub(crate) struct Lease {
    pub pool: Arc<Pool>,
    pub member_id: u64,
    pub epoch: u64,
    pub session: Arc<dyn Session>,
    class: JobClass,
    uses_control: bool,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut s = self.pool.lock();
        if self.uses_control {
            if let Some(m) = s
                .members
                .iter_mut()
                .find(|m| m.id == self.member_id && m.epoch == self.epoch)
            {
                if m.state == MemberState::Busy {
                    m.state = MemberState::Ready;
                }
            }
        }
        s.running = s.running.saturating_sub(1);
        if !self.uses_control {
            s.maintenance_running = s.maintenance_running.saturating_sub(1);
        }
        if self.class == JobClass::Maintenance {
            s.maintenance_active = false;
        }
        drop(s);
        self.pool.changed.notify_waiters();
    }
}

fn reclaim_exited_affinity(affinity: &mut HashMap<std::thread::ThreadId, ThreadAffinity>) {
    affinity.retain(|_, bound| bound.owner.strong_count() != 0);
}

fn observe_health(state: &mut PoolState) {
    let mut retired = Vec::new();
    for member in &mut state.members {
        if member.state == MemberState::Ready
            && !member
                .session
                .as_ref()
                .is_some_and(|session| session.is_healthy())
        {
            member.state = MemberState::Failed;
            member.last_error = ErrorCode::Disconnected;
            retired.push((member.id, member.epoch));
        }
    }
    for retired in retired {
        if !state.uncertain.contains(&retired) {
            state.uncertain.push(retired);
        }
    }
}

pub(crate) struct MaintenanceBlock {
    pool: Arc<Pool>,
}
impl MaintenanceBlock {
    pub fn new(pool: Arc<Pool>) -> Self {
        pool.lock().maintenance_blocked = true;
        Self { pool }
    }
}
impl Drop for MaintenanceBlock {
    fn drop(&mut self) {
        self.pool.lock().maintenance_blocked = false;
        self.pool.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exited_thread_affinity_is_reclaimed_but_live_affinity_is_retained() {
        let live_owner = Arc::new(());
        let live_thread = std::thread::current().id();
        let (exited_thread, exited_owner) = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let owner = Arc::new(());
                    (std::thread::current().id(), Arc::downgrade(&owner))
                })
                .join()
                .unwrap()
        });
        let mut affinity = HashMap::from([
            (
                live_thread,
                ThreadAffinity {
                    member_id: 1,
                    owner: Arc::downgrade(&live_owner),
                },
            ),
            (
                exited_thread,
                ThreadAffinity {
                    member_id: 2,
                    owner: exited_owner,
                },
            ),
        ]);
        reclaim_exited_affinity(&mut affinity);
        assert_eq!(affinity.len(), 1);
        assert_eq!(affinity[&live_thread].member_id, 1);
    }
}
