//! Enums de dominio con los valores **exactos** de DB (`TEXT` + `CHECK`).
//! El test `values_match_db_spec` compara cada enum con `docs/spec/symphony_database.md`.
//! Solo están los que el core ya usa; el resto se agrega en la fase que lo necesite.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("valor `{value}` inválido para {kind}")]
pub struct InvalidValue {
    pub kind: &'static str,
    pub value: String,
}

macro_rules! db_enum {
    ($(#[$doc:meta])* $name:ident { $($var:ident => $s:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $s)] $var),+
        }

        impl $name {
            /// Todos los valores, en el orden de DB.
            pub const ALL: &'static [Self] = &[$(Self::$var),+];

            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$var => $s),+ }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = InvalidValue;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($s => Ok(Self::$var),)+
                    _ => Err(InvalidValue { kind: stringify!($name), value: s.to_string() }),
                }
            }
        }
    };
}

db_enum!(
    /// `agents.state` (DB §3.C).
    AgentState {
        Created => "CREATED",
        Ready => "READY",
        Running => "RUNNING",
        WaitingProvider => "WAITING_PROVIDER",
        WaitingResource => "WAITING_RESOURCE",
        WaitingDependency => "WAITING_DEPENDENCY",
        Testing => "TESTING",
        Blocked => "BLOCKED",
        Paused => "PAUSED",
        Completed => "COMPLETED",
        Failed => "FAILED",
        Cancelled => "CANCELLED",
    }
);

db_enum!(
    /// `tasks.status` (DB §3.B).
    TaskStatus {
        Backlog => "BACKLOG",
        Ready => "READY",
        Running => "RUNNING",
        Waiting => "WAITING",
        Blocked => "BLOCKED",
        Done => "DONE",
        Failed => "FAILED",
        Cancelled => "CANCELLED",
    }
);

db_enum!(
    /// `agent_runs.status` (DB §3.C).
    RunStatus {
        Starting => "STARTING",
        Running => "RUNNING",
        Exited => "EXITED",
        Killed => "KILLED",
        Failed => "FAILED",
        HandedOff => "HANDED_OFF",
    }
);

db_enum!(
    /// `agent_runs.end_reason` (DB §3.C).
    RunEndReason {
        Completed => "COMPLETED",
        QuotaExhausted => "QUOTA_EXHAUSTED",
        RateLimited => "RATE_LIMITED",
        AuthError => "AUTH_ERROR",
        Crash => "CRASH",
        NoHeartbeat => "NO_HEARTBEAT",
        UserSwitch => "USER_SWITCH",
        UserStop => "USER_STOP",
    }
);

db_enum!(
    /// `agents.execution_mode`.
    ExecutionMode {
        Exact => "EXACT",
        Profile => "PROFILE",
        DecideLater => "DECIDE_LATER",
    }
);

db_enum!(
    /// `agents.failover_policy`.
    FailoverPolicy {
        None => "NONE",
        SameProvider => "SAME_PROVIDER",
        Any => "ANY",
    }
);

db_enum!(
    /// `agents.context_mode`.
    ContextMode {
        Raw => "RAW",
        Safe => "SAFE",
        Balanced => "BALANCED",
        Aggressive => "AGGRESSIVE",
    }
);

db_enum!(
    /// Perfil de rendimiento (DB `performance_profile`, FLOW §10.3).
    PerformanceProfile {
        Eco => "ECO",
        Balanced => "BALANCED",
        Performance => "PERFORMANCE",
        Custom => "CUSTOM",
    }
);

db_enum!(
    /// `provider_health.state` (DB §3.D).
    ProviderState {
        Healthy => "HEALTHY",
        Degraded => "DEGRADED",
        Throttled => "THROTTLED",
        RateLimited => "RATE_LIMITED",
        QuotaLow => "QUOTA_LOW",
        Exhausted => "EXHAUSTED",
        AuthError => "AUTH_ERROR",
        Offline => "OFFLINE",
        Unknown => "UNKNOWN",
        Probing => "PROBING",
    }
);

db_enum!(
    /// `provider_health.quota_certainty`.
    QuotaCertainty {
        Known => "KNOWN",
        Estimated => "ESTIMATED",
        Unknown => "UNKNOWN",
    }
);

db_enum!(
    /// `provider_failures.failure_type`.
    FailureType {
        Rpm => "RPM",
        Tpm => "TPM",
        TempRateLimit => "TEMP_RATE_LIMIT",
        DailyQuota => "DAILY_QUOTA",
        WeeklyQuota => "WEEKLY_QUOTA",
        ModelLimit => "MODEL_LIMIT",
        AccountLimit => "ACCOUNT_LIMIT",
        Auth => "AUTH",
        Network => "NETWORK",
        ProviderError => "PROVIDER_ERROR",
        ModelUnavailable => "MODEL_UNAVAILABLE",
        Unknown => "UNKNOWN",
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    const DB_SPEC: &str = include_str!("../../../docs/spec/symphony_database.md");

    /// Valores entre backticks de una fila de tabla de DB, en orden.
    fn backticked(line: &str) -> Vec<&str> {
        line.split('`').skip(1).step_by(2).collect()
    }

    fn assert_matches_db<T: Copy>(all: &[T], as_str: fn(T) -> &'static str, name: &str) {
        let ours: Vec<&str> = all.iter().map(|v| as_str(*v)).collect();
        let found = DB_SPEC
            .lines()
            .any(|l| l.starts_with('|') && backticked(l) == ours);
        assert!(found, "{name}: {ours:?} no coincide con ninguna fila de DB");
    }

    #[test]
    fn values_match_db_spec() {
        assert_matches_db(AgentState::ALL, AgentState::as_str, "AgentState");
        assert_matches_db(TaskStatus::ALL, TaskStatus::as_str, "TaskStatus");
        assert_matches_db(RunStatus::ALL, RunStatus::as_str, "RunStatus");
        assert_matches_db(RunEndReason::ALL, RunEndReason::as_str, "RunEndReason");
        assert_matches_db(ExecutionMode::ALL, ExecutionMode::as_str, "ExecutionMode");
        assert_matches_db(
            FailoverPolicy::ALL,
            FailoverPolicy::as_str,
            "FailoverPolicy",
        );
        assert_matches_db(ContextMode::ALL, ContextMode::as_str, "ContextMode");
        assert_matches_db(
            PerformanceProfile::ALL,
            PerformanceProfile::as_str,
            "PerformanceProfile",
        );
        assert_matches_db(ProviderState::ALL, ProviderState::as_str, "ProviderState");
        assert_matches_db(
            QuotaCertainty::ALL,
            QuotaCertainty::as_str,
            "QuotaCertainty",
        );
        assert_matches_db(FailureType::ALL, FailureType::as_str, "FailureType");
        assert_eq!(AgentState::ALL.len(), 12);
    }

    #[test]
    fn str_and_json_roundtrip() {
        for s in AgentState::ALL {
            assert_eq!(s.as_str().parse::<AgentState>().unwrap(), *s);
            let json = serde_json::to_string(s).unwrap();
            assert_eq!(json, format!("\"{}\"", s.as_str()));
            assert_eq!(serde_json::from_str::<AgentState>(&json).unwrap(), *s);
        }
        assert!("running".parse::<AgentState>().is_err());
        assert!(serde_json::from_str::<TaskStatus>("\"FINISHED\"").is_err());
    }
}
