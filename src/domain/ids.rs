use std::fmt;
use std::str::FromStr;

use super::DomainError;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident, $label:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(i64);

        impl $name {
            pub const fn new(value: i64) -> Self {
                Self(value)
            }

            pub const fn value(self) -> i64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                input.parse::<i64>().map(Self).map_err(|_| {
                    DomainError::InvalidIdentifier {
                        kind: $label,
                        input: input.to_owned(),
                    }
                })
            }
        }
    };
}

define_id!(
    /// Identifies an account in the ledger.
    AccountId,
    "account"
);
define_id!(
    /// Identifies a transaction header.
    TransactionId,
    "transaction"
);
define_id!(
    /// Identifies a split allocation row.
    AllocationId,
    "allocation"
);
define_id!(
    /// Identifies a spending/income category.
    CategoryId,
    "category"
);
define_id!(
    /// Identifies an ordered category group.
    CategoryGroupId,
    "category group"
);
define_id!(
    /// Identifies stored receipt/email evidence.
    EvidenceId,
    "evidence"
);
define_id!(
    /// Identifies a recurrence template.
    ScheduleId,
    "schedule"
);
define_id!(
    /// Identifies a provider connection.
    ConnectionId,
    "connection"
);
define_id!(
    /// Identifies an imported provider source record.
    SourceId,
    "source"
);
define_id!(
    /// Identifies a durable background job row.
    JobId,
    "job"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_numeric_and_distinctly_named() {
        let account: AccountId = "42".parse().unwrap();
        let transaction: TransactionId = "42".parse().unwrap();
        assert_eq!(account.value(), 42);
        assert_eq!(transaction.to_string(), "42");
        assert_eq!(
            "not-a-number".parse::<AccountId>(),
            Err(DomainError::InvalidIdentifier {
                kind: "account",
                input: "not-a-number".to_owned()
            })
        );
    }
}
