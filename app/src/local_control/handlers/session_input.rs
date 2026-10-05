//! Handler for `session.send_input`, the only local-control action that submits terminal input.
//!
//! Upstream `warpctrl` deliberately never submits an input buffer. This fork accepts that
//! deviation under strict limits: input goes only to sessions that a local-control `tab.create`
//! request created, only as a single line whose bytes cannot escape bracketed paste or inject
//! control sequences, and only while the caller's expected user-input count still matches. That
//! count lets a caller stage text, wait, and submit on a later request; any user keystroke in
//! between changes the count and the submit is refused instead of landing in the user's typing.
#[cfg(test)]
#[path = "session_input_tests.rs"]
mod tests;

use ::local_control::protocol::{SessionSendInputParams, SessionTarget, TargetSelector};
use ::local_control::{ActionKind, ControlError, ErrorCode};
use serde::Serialize;
use warpui::ModelContext;

use crate::local_control::LocalControlBridge;
use crate::local_control::resolver::{decode_params, explicit_session_terminal_view};
use crate::terminal::model::escape_sequences::{BRACKETED_PASTE_END, BRACKETED_PASTE_START};
use crate::terminal::model::grid::grid_handler::TermMode;

const ACTION: ActionKind = ActionKind::SessionSendInput;

/// Largest UTF-8 byte length accepted for `text`.
const MAX_TEXT_BYTES: usize = 16384;

#[derive(Serialize)]
struct SessionSendInputResponse<'a> {
    action: &'static str,
    session_id: &'a str,
    wrote_text: bool,
    submitted: bool,
    bracketed_paste: bool,
    user_input_count: u64,
}

/// Sends text and/or a submit keystroke to a local-control session.
///
/// Every check runs before the first byte is written, and each write goes through
/// [`TerminalView::write_user_bytes_to_pty_inner`] with `counts_as_user_input` set to `false`, so
/// the request cannot move the counter it just compared against.
///
/// [`TerminalView::write_user_bytes_to_pty_inner`]: crate::terminal::view::TerminalView::write_user_bytes_to_pty_inner
pub(crate) fn send_input(
    params: &serde_json::Value,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let action = ACTION.as_str();
    let params = decode_params::<SessionSendInputParams>(params)?;
    validate_send_input_params(&params)?;

    let terminal_view = explicit_session_terminal_view(ACTION, target, ctx)?;
    let Some(SessionTarget::Id { id }) = target.session.as_ref() else {
        // `explicit_session_terminal_view` rejects every other session selector.
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action} requires an explicit session id"),
        ));
    };
    let session_id = id.0.clone();

    // `is_long_running` locks the terminal model itself, so it runs in its own read and never
    // under the model guard acquired below.
    let long_running = terminal_view.read(ctx, |terminal_view, _| terminal_view.is_long_running());
    let (created_by_local_control, agent_in_control, user_input_count, bracketed) = terminal_view
        .read(ctx, |terminal_view, _| {
            let model = terminal_view.model.lock();
            (
                terminal_view.created_by_local_control(),
                model.block_list().active_block().is_agent_in_control(),
                terminal_view.user_input_count(),
                model.is_term_mode_set(TermMode::BRACKETED_PASTE),
            )
        });

    if !created_by_local_control {
        return Err(ControlError::new(
            ErrorCode::InsufficientPermissions,
            format!("{action} only writes to sessions created by tab.create through local control"),
        ));
    }
    if !long_running {
        return Err(ControlError::new(
            ErrorCode::TargetStateConflict,
            format!("{action} requires a running long command to receive the text"),
        ));
    }
    if agent_in_control {
        return Err(ControlError::new(
            ErrorCode::TargetStateConflict,
            format!("{action} cannot write while Warp's own agent controls the active block"),
        ));
    }
    if user_input_count != params.expected_user_input_count {
        return Err(ControlError::with_details(
            ErrorCode::TargetStateConflict,
            format!("{action} found user input since the caller last observed the session"),
            format!("current_user_input_count={user_input_count}"),
        ));
    }

    let wrote_text = params.text.is_some();
    if let Some(text) = &params.text {
        let mut bytes = Vec::with_capacity(
            text.len() + BRACKETED_PASTE_START.len() + BRACKETED_PASTE_END.len(),
        );
        if bracketed {
            bytes.extend_from_slice(BRACKETED_PASTE_START);
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(BRACKETED_PASTE_END);
        } else {
            bytes.extend_from_slice(text.as_bytes());
        }
        terminal_view.update(ctx, |terminal_view, ctx| {
            terminal_view.write_user_bytes_to_pty_inner(bytes, false, ctx);
        });
    }
    if params.submit {
        terminal_view.update(ctx, |terminal_view, ctx| {
            terminal_view.write_user_bytes_to_pty_inner(vec![b'\r'], false, ctx);
        });
    }

    serde_json::to_value(SessionSendInputResponse {
        action,
        session_id: &session_id,
        wrote_text,
        submitted: params.submit,
        bracketed_paste: bracketed,
        user_input_count,
    })
    .map_err(|error| {
        ControlError::with_details(
            ErrorCode::Internal,
            "failed to serialize session.send_input response",
            error.to_string(),
        )
    })
}

/// Validates `session.send_input` parameters before any target or session state is touched.
///
/// `text` must be non-empty and free of C0 control characters and DEL. That restriction keeps the
/// payload from closing an open bracketed paste early or injecting an escape sequence that a
/// terminal program would interpret as input of its own.
pub(crate) fn validate_send_input_params(
    params: &SessionSendInputParams,
) -> Result<(), ControlError> {
    let action = ACTION.as_str();
    if params.text.is_none() && !params.submit {
        return Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!("{action} requires at least one of text or submit"),
        ));
    }
    let Some(text) = &params.text else {
        return Ok(());
    };
    if text.is_empty() {
        return Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!("{action} rejects empty text"),
        ));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!("{action} rejects text longer than {MAX_TEXT_BYTES} UTF-8 bytes"),
        ));
    }
    if let Some(character) = text
        .chars()
        .find(|character| character.is_control() || *character == '\u{7f}')
    {
        return Err(ControlError::with_details(
            ErrorCode::InvalidParams,
            format!("{action} rejects control characters in text"),
            format!("rejected_character=U+{:04X}", character as u32),
        ));
    }
    Ok(())
}
