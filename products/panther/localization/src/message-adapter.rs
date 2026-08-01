// @file products/panther/localization/src/message-adapter.rs
// @description Defines the typed-state to message-key adapter pattern.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::message_arguments::MessageArguments;

/// Maps a typed state to a message key and its arguments.
///
/// Low-level crates report typed states and stable codes, never prose. The
/// Panther presentation boundary turns a typed state into a message through an
/// exhaustive mapping: a stable message id plus the named arguments the
/// template needs, supplied as data. This trait fixes that shape; concrete
/// adapters arrive with the states they map in later phases.
pub trait MessageAdapter {
    /// Returns the stable message id for this state.
    fn message_id(&self) -> &'static str;

    /// Returns the named arguments the message template needs.
    ///
    /// The default is an empty set for states whose message takes no arguments.
    fn arguments(&self) -> MessageArguments {
        MessageArguments::new()
    }
}

#[cfg(test)]
mod tests {
    use super::MessageAdapter;
    use crate::message_arguments::{MessageArgument, MessageArguments};

    enum SampleState {
        Granted,
        Denied { attempts: i64 },
    }

    impl MessageAdapter for SampleState {
        fn message_id(&self) -> &'static str {
            match self {
                SampleState::Granted => "sample.granted",
                SampleState::Denied { .. } => "sample.denied",
            }
        }

        fn arguments(&self) -> MessageArguments {
            match self {
                SampleState::Granted => MessageArguments::new(),
                SampleState::Denied { attempts } => {
                    MessageArguments::new().with("attempts", MessageArgument::Integer(*attempts))
                }
            }
        }
    }

    #[test]
    fn adapter_maps_state_to_key_and_arguments() {
        assert_eq!(SampleState::Granted.message_id(), "sample.granted");
        assert!(SampleState::Granted.arguments().entries().is_empty());

        let denied = SampleState::Denied { attempts: 3 };
        assert_eq!(denied.message_id(), "sample.denied");
        assert_eq!(
            denied.arguments().entries(),
            &[("attempts", MessageArgument::Integer(3))]
        );
    }
}
