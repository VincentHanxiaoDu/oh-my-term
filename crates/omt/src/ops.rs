//! Operating the instance: jobs, shutdown, service units, upgrade, remotes.
//!
//! `docs/architecture/22-operations.md` §10.
//!
//! **On the name.** §10.1 calls the long-running-operation registry `job.*`,
//! but `job.list` is already taken by the scheduled jobs of 20 §12 — two
//! different things that the document set gives one name. They are declared
//! here as `task.*`: a capability name is resolved as a whole string and two
//! meanings under one name would be resolved by load order, which is worse than
//! a name that differs from one table.
//!
//! **Cancellation is cooperative and says so.** A job stops at its next
//! checkpoint. One that has passed its point of no return — an install that has
//! begun the atomic rename — refuses with the reason rather than reporting a
//! success it cannot deliver. A cancel that lies is worse than a cancel that
//! refuses, because the caller stops watching.

use omt_catalog::{CapabilityError, CapabilityHandler};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Where a long-running operation has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Still going.
    Running,
    /// Asked to stop, and it has not reached a checkpoint yet.
    Cancelling,
    /// Stopped because it was asked to.
    Cancelled,
    /// Finished.
    Done,
    /// Finished badly.
    Failed,
}

impl TaskState {
    /// Whether this is a state a task can leave.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Cancelled | Self::Done | Self::Failed)
    }
}

/// One long-running operation.
#[derive(Debug, Clone, Serialize)]
pub struct Task {
    /// Its id.
    pub id: String,
    /// Which capability is running.
    pub capability: String,
    /// Who asked.
    pub actor: String,
    /// When it began, in milliseconds.
    pub started_at_ms: u64,
    /// How far along, from 0 to 100 where it can say.
    pub progress: Option<u8>,
    /// Where it is.
    pub state: TaskState,
    /// Whether it can still be stopped.
    ///
    /// False once the operation has done something it cannot undo — an atomic
    /// rename that has begun, bytes already written over a binary.
    pub cancellable: bool,
    /// What happened, once it is over.
    pub outcome: Option<String>,
}

/// The running tasks.
pub type Tasks = Arc<Mutex<BTreeMap<String, Task>>>;

/// Serves the task capabilities.
pub struct TaskHandler(pub Tasks);

/// Input to `task.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskListIn {
    /// Include the ones that have finished.
    #[serde(default)]
    pub include_finished: bool,
}

/// One task, as reported.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskSummary {
    /// Its id.
    pub id: String,
    /// Which capability.
    pub capability: String,
    /// Who asked.
    pub actor: String,
    /// How far along.
    pub progress: Option<u8>,
    /// Where it is.
    pub state: String,
    /// Whether it can still be stopped.
    pub cancellable: bool,
}

/// What `task.list` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskListOut {
    /// The tasks.
    pub tasks: Vec<TaskSummary>,
}

fn summary(task: &Task) -> TaskSummary {
    TaskSummary {
        id: task.id.clone(),
        capability: task.capability.clone(),
        actor: task.actor.clone(),
        progress: task.progress,
        state: serde_json::to_value(task.state)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "running".to_owned()),
        cancellable: task.cancellable,
    }
}

omt_catalog::capability! {
    /// Long-running operations and where they have got to.
    pub struct TaskList;
    input  = TaskListIn,
    output = TaskListOut,
    decl = omt_catalog::Decl {
        name: "task.list",
        group: "task",
        verb: "list",
        title: "List running operations",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "22 §10.1's job registry. Named `task` because `job` already means a scheduled job, and one name for two things would resolve by load order.",
    },
}

impl CapabilityHandler<TaskList> for TaskHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: TaskListIn,
    ) -> Result<TaskListOut, CapabilityError> {
        let tasks = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the task lock was poisoned"))?;
        Ok(TaskListOut {
            tasks: tasks
                .values()
                .filter(|t| input.include_finished || !t.state.is_final())
                .map(summary)
                .collect(),
        })
    }
}

/// Input to `task.get`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskGetIn {
    /// Which task.
    pub task: String,
}

/// What `task.get` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskGetOut {
    /// The task.
    pub task: TaskSummary,
    /// What happened, once it is over.
    pub outcome: Option<String>,
}

omt_catalog::capability! {
    /// One long-running operation.
    pub struct TaskGet;
    input  = TaskGetIn,
    output = TaskGetOut,
    decl = omt_catalog::Decl {
        name: "task.get",
        group: "task",
        verb: "get",
        title: "Get a running operation",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Where one operation got to, so a client that reconnected mid-job learns the answer it can no longer hear.",
    },
}

impl CapabilityHandler<TaskGet> for TaskHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: TaskGetIn,
    ) -> Result<TaskGetOut, CapabilityError> {
        let tasks = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the task lock was poisoned"))?;
        let task = tasks
            .get(&input.task)
            .ok_or_else(|| CapabilityError::not_found(format!("no task {}", input.task)))?;
        Ok(TaskGetOut {
            task: summary(task),
            outcome: task.outcome.clone(),
        })
    }
}

/// Input to `task.cancel`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskCancelIn {
    /// Which task.
    pub task: String,
}

/// What `task.cancel` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskCancelOut {
    /// Where it is now.
    ///
    /// `cancelling`, not `cancelled`: it stops at its next checkpoint, and
    /// saying otherwise would have the caller believe it had already stopped.
    pub state: String,
}

omt_catalog::capability! {
    /// Ask a long-running operation to stop.
    pub struct TaskCancel;
    input  = TaskCancelIn,
    output = TaskCancelOut,
    decl = omt_catalog::Decl {
        name: "task.cancel",
        group: "task",
        verb: "cancel",
        title: "Cancel an operation",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        // Cas: asking twice is asking once. A repeated cancel must not be able
        // to stop whatever took the id next.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Cooperative: it stops at its next checkpoint. One past its point of no return refuses with the reason rather than reporting a success it cannot deliver.",
    },
}

impl CapabilityHandler<TaskCancel> for TaskHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: TaskCancelIn,
    ) -> Result<TaskCancelOut, CapabilityError> {
        let mut tasks = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the task lock was poisoned"))?;
        let task = tasks
            .get_mut(&input.task)
            .ok_or_else(|| CapabilityError::not_found(format!("no task {}", input.task)))?;
        if task.state.is_final() {
            // Not an error: the caller wanted it stopped and it is stopped.
            return Ok(TaskCancelOut {
                state: summary(task).state,
            });
        }
        if !task.cancellable {
            return Err(CapabilityError::precondition_failed(
                "that operation is past the point where it can be undone; it will finish",
            ));
        }
        task.state = TaskState::Cancelling;
        Ok(TaskCancelOut {
            state: "cancelling".to_owned(),
        })
    }
}

/// Input to `upgrade.check`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpgradeCheckIn {
    /// Which channel to look at.
    #[serde(default)]
    pub channel: Option<String>,
}

/// What `upgrade.check` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct UpgradeCheckOut {
    /// What is running.
    pub current: String,
    /// What is available, where omt could find out.
    pub available: Option<String>,
    /// How omt was installed, and therefore what should do the upgrading.
    pub installed_by: String,
    /// The command to run instead, when a package manager owns the binary.
    pub delegate_to: Option<String>,
    /// What is running that would be lost by restarting now.
    pub blocked_by: Vec<String>,
}

omt_catalog::capability! {
    /// Whether there is a newer omt, asked because you asked.
    pub struct UpgradeCheck;
    input  = UpgradeCheckIn,
    output = UpgradeCheckOut,
    decl = omt_catalog::Decl {
        name: "upgrade.check",
        group: "upgrade",
        verb: "check",
        title: "Check for an upgrade",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        // It reaches the network, and only because somebody asked: there is no
        // background poll, so this effect is never incurred behind the user's
        // back.
        effects: omt_catalog::Effects::NETWORK,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "omt never checks on its own. This reaches the network because the user typed it, and reports who owns the binary so a package manager's file is delegated to rather than overwritten.",
    },
}

/// Serves the upgrade capabilities.
pub struct UpgradeHandler(pub crate::state::State);

impl CapabilityHandler<UpgradeCheck> for UpgradeHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: UpgradeCheckIn,
    ) -> Result<UpgradeCheckOut, CapabilityError> {
        let installed = std::env::current_exe().map_or(crate::upgrade::Installed::Plain, |path| {
            crate::upgrade::installed_from(&path.display().to_string())
        });
        let blocked_by = busy_sessions(&self.0)?;
        Ok(UpgradeCheckOut {
            current: env!("CARGO_PKG_VERSION").to_owned(),
            // No release feed is wired up yet, and inventing one would be a
            // network call to somewhere omt does not have.
            available: None,
            installed_by: format!("{installed:?}").to_lowercase(),
            delegate_to: installed.delegate_to().map(str::to_owned),
            blocked_by,
        })
    }
}

/// Sessions whose agent is working, by id.
fn busy_sessions(state: &crate::state::State) -> Result<Vec<String>, CapabilityError> {
    let instance = state.lock()?;
    let mut busy = Vec::new();
    for session in instance.sessions() {
        let id = session.id;
        if instance.threads(id).is_some_and(|roster| {
            roster.threads().iter().any(|t| {
                matches!(
                    t.state,
                    omt_types::AgentState::Working { .. } | omt_types::AgentState::Blocked { .. }
                )
            })
        }) {
            busy.push(id.to_wire());
        }
    }
    Ok(busy)
}

/// Input to `instance.shutdown`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShutdownIn {
    /// Stop even if something is working.
    #[serde(default)]
    pub force: bool,
}

/// What `instance.shutdown` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ShutdownOut {
    /// Whether it is going down.
    pub shutting_down: bool,
    /// What stopped it, when it is not.
    pub blocked_by: Vec<String>,
}

omt_catalog::capability! {
    /// Stop this instance.
    pub struct InstanceShutdown;
    input  = ShutdownIn,
    output = ShutdownOut,
    decl = omt_catalog::Decl {
        name: "instance.shutdown",
        group: "instance",
        verb: "shutdown",
        title: "Shut down",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::DESTRUCTIVE,
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Refuses while an agent is working or blocked, naming the sessions. A shutdown that takes an agent down mid-turn is the same loss an upgrade refuses to cause.",
    },
}

/// Serves the instance lifecycle capabilities.
pub struct LifecycleHandler(pub crate::state::State);

impl CapabilityHandler<InstanceShutdown> for LifecycleHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ShutdownIn,
    ) -> Result<ShutdownOut, CapabilityError> {
        let blocked_by = busy_sessions(&self.0)?;
        if !blocked_by.is_empty() && !input.force {
            // Reported rather than raised: the caller asked a question and the
            // answer is "these sessions", which is a result and not a fault.
            return Ok(ShutdownOut {
                shutting_down: false,
                blocked_by,
            });
        }
        Ok(ShutdownOut {
            shutting_down: true,
            blocked_by: Vec::new(),
        })
    }
}

/// Input to `remote.probe`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoteProbeIn {
    /// Which host, as ssh would name it.
    pub host: String,
}

/// What `remote.probe` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RemoteProbeOut {
    /// Whether omt is there.
    pub present: bool,
    /// What the host said it is.
    pub uname: Option<String>,
    /// Which artifact it would take.
    pub artifact: Option<String>,
    /// Whether it can decompress.
    pub has_zstd: bool,
    /// What omt would do about it.
    pub decision: String,
}

omt_catalog::capability! {
    /// Ask a host whether it has omt, and what it would need.
    pub struct RemoteProbe;
    input  = RemoteProbeIn,
    output = RemoteProbeOut,
    decl = omt_catalog::Decl {
        name: "remote.probe",
        group: "remote",
        verb: "probe",
        title: "Probe a host",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        // Operator rather than Viewer, matching its sibling: it opens an
        // outbound connection to a host the caller named, and a read-only
        // shared link must not be able to make this machine connect somewhere.
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::NETWORK,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Operator, not Viewer, because it makes this machine reach out to a host the caller chose. Viewer with NETWORK has no carve-out.",
    },
}

/// Serves `remote.probe`.
pub struct RemoteHandler;

impl CapabilityHandler<RemoteProbe> for RemoteHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RemoteProbeIn,
    ) -> Result<RemoteProbeOut, CapabilityError> {
        // The same one exec that a real attach uses: probe and connect are not
        // two round trips, so a host with MFA prompts once.
        let script = r#"omt serve --stdio --proto 1 2>/dev/null || { printf "OMT-MISSING "; uname -sm; command -v zstd >/dev/null && echo zstd || echo nozstd; }"#;
        let out = std::process::Command::new("ssh")
            .arg("-o")
            .arg("BatchMode=yes")
            .arg(&input.host)
            .arg("--")
            .arg(script)
            .output()
            .map_err(|e| CapabilityError::precondition_failed(format!("could not run ssh: {e}")))?;

        let text = String::from_utf8_lossy(&out.stdout);
        let Some(probe) = crate::bootstrap::Probe::parse(text.trim()) else {
            if out.status.success() {
                // It answered with a protocol frame rather than the marker:
                // omt is there.
                return Ok(RemoteProbeOut {
                    present: true,
                    uname: None,
                    artifact: None,
                    has_zstd: false,
                    decision: "already installed".to_owned(),
                });
            }
            return Err(CapabilityError::precondition_failed(format!(
                "{} did not answer the probe",
                input.host
            )));
        };

        let target = crate::bootstrap::target(&probe);
        let decision = crate::bootstrap::decide(crate::bootstrap::Consent::Unknown, &target);
        Ok(RemoteProbeOut {
            present: false,
            uname: Some(format!("{} {}", probe.system, probe.machine)),
            artifact: match &target {
                crate::bootstrap::Target::Artifact(name) => Some((*name).to_owned()),
                crate::bootstrap::Target::Unsupported { .. } => None,
            },
            has_zstd: probe.zstd,
            decision: match decision {
                crate::bootstrap::Decision::Ask => "would ask before installing".to_owned(),
                crate::bootstrap::Decision::Install => "would install".to_owned(),
                crate::bootstrap::Decision::Degrade { notice } => notice,
            },
        })
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn task(cancellable: bool, state: TaskState) -> Task {
        Task {
            id: "task_1".to_owned(),
            capability: "upgrade.apply".to_owned(),
            actor: "local".to_owned(),
            started_at_ms: 0,
            progress: Some(40),
            state,
            cancellable,
            outcome: None,
        }
    }

    fn registry(task: Task) -> Tasks {
        let mut map = BTreeMap::new();
        map.insert(task.id.clone(), task);
        Arc::new(Mutex::new(map))
    }

    fn ctx() -> omt_catalog::CallContext {
        omt_catalog::CallContext {
            actor: omt_types::Actor::Local,
            role: omt_types::Role::Admin,
            request: omt_catalog::RequestId {
                device: omt_types::DeviceId::new(),
                n: 1,
            },
            intent: None,
        }
    }

    #[test]
    fn a_cancel_reports_cancelling_rather_than_cancelled() {
        // It stops at its next checkpoint. Saying `cancelled` would have the
        // caller believe it had already stopped.
        let tasks = registry(task(true, TaskState::Running));
        let out = CapabilityHandler::<TaskCancel>::call(
            &TaskHandler(tasks),
            &ctx(),
            TaskCancelIn {
                task: "task_1".to_owned(),
            },
        )
        .expect("cancelled");
        assert_eq!(out.state, "cancelling");
    }

    #[test]
    fn a_task_past_its_point_of_no_return_refuses_rather_than_lying() {
        // A cancel that reports a success it cannot deliver is worse than one
        // that refuses, because the caller stops watching.
        let tasks = registry(task(false, TaskState::Running));
        let error = CapabilityHandler::<TaskCancel>::call(
            &TaskHandler(tasks),
            &ctx(),
            TaskCancelIn {
                task: "task_1".to_owned(),
            },
        )
        .expect_err("refused");
        assert_eq!(error.code, omt_catalog::ErrorCode::PreconditionFailed);
    }

    #[test]
    fn cancelling_something_already_finished_is_not_an_error() {
        // The caller wanted it stopped and it is stopped.
        let tasks = registry(task(true, TaskState::Done));
        let out = CapabilityHandler::<TaskCancel>::call(
            &TaskHandler(tasks),
            &ctx(),
            TaskCancelIn {
                    task: "task_1".to_owned(),
                },
            )
            .expect("not an error");
        assert_eq!(out.state, "done");
    }

    #[test]
    fn finished_tasks_are_left_out_unless_asked_for() {
        // A list that keeps every task ever run is one nobody reads.
        let tasks = registry(task(true, TaskState::Done));
        let hidden = CapabilityHandler::<TaskList>::call(
            &TaskHandler(tasks.clone()),
            &ctx(),
            TaskListIn {
                include_finished: false,
            },
        )
        .expect("listed");
        assert!(hidden.tasks.is_empty());
        let shown = CapabilityHandler::<TaskList>::call(
            &TaskHandler(tasks),
            &ctx(),
            TaskListIn {
                include_finished: true,
            },
        )
        .expect("listed");
        assert_eq!(shown.tasks.len(), 1);
    }

    #[test]
    fn a_task_that_does_not_exist_is_not_found() {
        let tasks = registry(task(true, TaskState::Running));
        let error = CapabilityHandler::<TaskGet>::call(
            &TaskHandler(tasks),
            &ctx(),
            TaskGetIn {
                task: "task_9".to_owned(),
            },
        )
        .expect_err("not found");
        assert_eq!(error.code, omt_catalog::ErrorCode::NotFound);
    }

    #[test]
    fn only_final_states_are_final() {
        // `Cancelling` is not: a task that has been asked to stop is still
        // running, and treating it as finished would hide it from the list
        // while it is still doing something.
        assert!(!TaskState::Running.is_final());
        assert!(!TaskState::Cancelling.is_final());
        assert!(TaskState::Cancelled.is_final());
        assert!(TaskState::Done.is_final());
        assert!(TaskState::Failed.is_final());
    }
}
