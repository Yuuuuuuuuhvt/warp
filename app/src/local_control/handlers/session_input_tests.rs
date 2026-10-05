use std::cell::RefCell;
use std::rc::Rc;

use ::local_control::protocol::{
    SessionSelector, SessionSendInputParams, SessionTarget, TargetSelector,
};
use ::local_control::{ControlError, ErrorCode};
use serde_json::json;
use warpui::{App, ModelHandle, ViewHandle};

use super::{ACTION, send_input, validate_send_input_params};
use crate::local_control::LocalControlBridge;
use crate::pane_group::PaneId;
use crate::terminal::model::grid::grid_handler::TermMode;
use crate::terminal::{Event, TerminalView};
use crate::test_util::terminal::{add_window_with_terminal, initialize_app_for_terminal_view};
use crate::workspace::view::tests::{initialize_app as initialize_workspace_app, mock_workspace};

/// Builds the target selector that names one session explicitly.
fn session_target(session_id: &str) -> TargetSelector {
    TargetSelector {
        session: Some(SessionTarget::Id {
            id: SessionSelector(session_id.to_owned()),
        }),
        ..Default::default()
    }
}

fn send_input_params(value: serde_json::Value) -> SessionSendInputParams {
    serde_json::from_value(value).expect("session.send_input params decode")
}

#[test]
fn send_input_rejects_missing_text_and_submit() {
    let error = validate_send_input_params(&send_input_params(json!({
        "expected_user_input_count": 0,
    })))
    .expect_err("at least one of text or submit is required");
    assert_eq!(error.code, ErrorCode::InvalidParams);
    assert!(error.message.contains("text or submit"));
}

#[test]
fn send_input_rejects_empty_text() {
    let error = validate_send_input_params(&send_input_params(json!({
        "text": "",
        "expected_user_input_count": 0,
    })))
    .expect_err("empty text is rejected");
    assert_eq!(error.code, ErrorCode::InvalidParams);
}

#[test]
fn send_input_rejects_oversized_text() {
    let text = "a".repeat(16385);
    let error = validate_send_input_params(&send_input_params(json!({
        "text": text,
        "expected_user_input_count": 0,
    })))
    .expect_err("text beyond the byte limit is rejected");
    assert_eq!(error.code, ErrorCode::InvalidParams);
    assert!(error.message.contains("16384"));
}

#[test]
fn send_input_rejects_control_characters() {
    for text in ["git status\nrm -rf .", "bracketed\u{1b}[201~", "del\u{7f}"] {
        let error = validate_send_input_params(&send_input_params(json!({
            "text": text,
            "expected_user_input_count": 0,
        })))
        .expect_err("control characters are rejected");
        assert_eq!(error.code, ErrorCode::InvalidParams, "{text:?}");
        assert!(error.message.contains("control characters"), "{text:?}");
    }
}

#[test]
fn send_input_accepts_printable_unicode_and_submit_only() {
    validate_send_input_params(&send_input_params(json!({
        "text": "echo \u{1f600}",
        "submit": true,
        "expected_user_input_count": 7,
    })))
    .expect("printable unicode and submit are accepted");

    validate_send_input_params(&send_input_params(json!({
        "submit": true,
        "expected_user_input_count": 7,
    })))
    .expect("submit-only input is accepted");
}

#[test]
fn send_input_rejects_active_and_unsupported_selectors() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _terminal = add_window_with_terminal(&mut app, None);
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        for target in [
            TargetSelector {
                session: Some(SessionTarget::Active),
                ..Default::default()
            },
            TargetSelector::default(),
        ] {
            let error = bridge.update(&mut app, |_, ctx| {
                send_input(
                    &json!({ "submit": true, "expected_user_input_count": 0 }),
                    &target,
                    ctx,
                )
                .expect_err("only an explicit session id is accepted")
            });
            assert_eq!(error.code, ErrorCode::InvalidSelector, "{target:?}");
        }
    });
}

/// A local-control session whose active block runs a long command, the only state
/// `session.send_input` writes to, plus a recorder for the bytes its PTY received.
struct LocalControlSession {
    terminal: ViewHandle<TerminalView>,
    bridge: ModelHandle<LocalControlBridge>,
    session_id: String,
    pty_writes: Rc<RefCell<Vec<Vec<u8>>>>,
}

fn local_control_session(app: &mut App, mark_created: bool) -> LocalControlSession {
    session_in_state(app, mark_created, true, true)
}

/// Builds a session whose bootstrap and long-command state the test chooses explicitly, so a
/// refusal test does not depend on the mock terminal's default state.
fn session_in_state(
    app: &mut App,
    mark_created: bool,
    bootstrapped: bool,
    long_running: bool,
) -> LocalControlSession {
    initialize_workspace_app(app);
    let workspace = mock_workspace(app);
    // `session.list` reports a terminal pane's session id as the pane id, and the resolver matches
    // the same id, so the test targets the session through the pane it belongs to.
    let (session_id, terminal) = workspace.read(app, |workspace, ctx| {
        let pane_group = workspace.active_tab_pane_group();
        let pane_id: PaneId = pane_group
            .read(ctx, |pane_group, ctx| {
                pane_group
                    .active_session_id(ctx)
                    .expect("a mock workspace has an active terminal session")
            })
            .into();
        let terminal = pane_group
            .read(ctx, |pane_group, ctx| pane_group.active_session_view(ctx))
            .expect("the active session has a terminal pane");
        (pane_id.to_string(), terminal)
    });

    terminal.update(app, |view, _| {
        let mut model = view.model.lock();
        if bootstrapped {
            model.block_list_mut().set_bootstrapped();
        }
        if long_running {
            model.simulate_long_running_block("sleep", "");
        }
    });
    if mark_created {
        terminal.update(app, |view, _| view.mark_created_by_local_control());
    }

    let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
    let recorder = pty_writes.clone();
    app.update(|ctx| {
        ctx.subscribe_to_view(&terminal, move |_, event, _| {
            if let Event::WriteBytesToPty { bytes } = event {
                recorder.borrow_mut().push(bytes.to_vec());
            }
        });
    });

    let bridge = app.add_singleton_model(LocalControlBridge::new);
    LocalControlSession {
        terminal,
        bridge,
        session_id,
        pty_writes,
    }
}

impl LocalControlSession {
    fn send(
        &self,
        app: &mut App,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ControlError> {
        let target = session_target(&self.session_id);
        self.bridge
            .update(app, |_, ctx| send_input(&params, &target, ctx))
    }

    fn user_input_count(&self, app: &App) -> u64 {
        self.terminal.read(app, |view, _| view.user_input_count())
    }
}

#[test]
fn send_input_reports_stale_target_for_unknown_session_ids() {
    App::test((), |mut app| async move {
        let session = local_control_session(&mut app, true);
        let unknown = format!("{}-missing", session.session_id);
        let target = session_target(&unknown);

        let error = session.bridge.update(&mut app, |_, ctx| {
            send_input(
                &json!({ "text": "ls", "expected_user_input_count": 0 }),
                &target,
                ctx,
            )
            .expect_err("an unknown session id is stale")
        });
        assert_eq!(error.code, ErrorCode::StaleTarget);
        assert!(session.pty_writes.borrow().is_empty());
    });
}

#[test]
fn send_input_rejects_sessions_not_created_by_local_control() {
    App::test((), |mut app| async move {
        let session = local_control_session(&mut app, false);

        let error = session
            .send(
                &mut app,
                json!({ "text": "ls", "expected_user_input_count": 0 }),
            )
            .expect_err("an unmarked session must be rejected");
        assert_eq!(error.code, ErrorCode::InsufficientPermissions);
        assert!(session.pty_writes.borrow().is_empty());
    });
}

#[test]
fn send_input_rejects_a_mismatched_user_input_count() {
    App::test((), |mut app| async move {
        let session = local_control_session(&mut app, true);
        // Start the count above zero so the assertion has to read the live value, not a default.
        session.terminal.update(&mut app, |view, ctx| {
            view.write_user_bytes_to_pty_inner(b"x".to_vec(), true, ctx);
        });
        assert_eq!(session.user_input_count(&app), 1);
        let writes_before = session.pty_writes.borrow().len();

        let error = session
            .send(
                &mut app,
                json!({ "text": "ls", "expected_user_input_count": 0 }),
            )
            .expect_err("a count mismatch must be rejected");
        assert_eq!(error.code, ErrorCode::TargetStateConflict);
        assert!(
            error
                .details
                .as_deref()
                .is_some_and(|details| details.contains("1"))
        );
        assert_eq!(
            session.pty_writes.borrow().len(),
            writes_before,
            "a rejected request must not reach the PTY"
        );
    });
}

#[test]
fn send_input_writes_text_and_submit_without_counting_them() {
    App::test((), |mut app| async move {
        let session = local_control_session(&mut app, true);

        let response = session
            .send(
                &mut app,
                json!({
                    "text": "git status",
                    "submit": true,
                    "expected_user_input_count": 0,
                }),
            )
            .expect("an unmodified created session accepts input");

        assert_eq!(response["action"], ACTION.as_str());
        assert_eq!(response["wrote_text"], true);
        assert_eq!(response["submitted"], true);
        assert_eq!(response["user_input_count"], 0);
        assert_eq!(response["bracketed_paste"], false);
        assert_eq!(response["session_id"], json!(session.session_id));

        assert_eq!(
            session.pty_writes.borrow().as_slice(),
            [b"git status".to_vec(), b"\r".to_vec()].as_slice()
        );
        assert_eq!(
            session.user_input_count(&app),
            0,
            "session.send_input writes must not move the counter it compared against"
        );
    });
}

#[test]
fn send_input_wraps_bracketed_paste_when_the_session_requested_it() {
    App::test((), |mut app| async move {
        let session = local_control_session(&mut app, true);
        session.terminal.update(&mut app, |view, _| {
            view.model.lock().process_bytes("\u{1b}[?2004h");
        });
        assert!(session.terminal.read(&app, |view, _| {
            view.model
                .lock()
                .is_term_mode_set(TermMode::BRACKETED_PASTE)
        }));

        let response = session
            .send(
                &mut app,
                json!({ "text": "ls", "expected_user_input_count": 0 }),
            )
            .expect("a bracketed-paste session accepts input");

        assert_eq!(response["bracketed_paste"], true);
        assert_eq!(
            session.pty_writes.borrow().as_slice(),
            [b"\x1b[200~ls\x1b[201~".to_vec()].as_slice()
        );
    });
}

#[test]
fn send_input_refuses_a_session_that_is_still_bootstrapping() {
    App::test((), |mut app| async move {
        let session = session_in_state(&mut app, true, false, true);
        assert!(
            !session.terminal.read(&app, |view, _| view
                .model
                .lock()
                .block_list()
                .is_bootstrapped()),
            "the test needs a session whose shell has not finished bootstrapping"
        );

        let error = session
            .send(
                &mut app,
                json!({ "text": "task", "expected_user_input_count": 0 }),
            )
            .expect_err("a bootstrapping shell must not receive text");

        assert_eq!(error.code, ErrorCode::TargetStateConflict);
        assert!(error.message.contains("finish starting"));
        assert!(session.pty_writes.borrow().is_empty());
    });
}

#[test]
fn send_input_refuses_a_session_without_a_long_command() {
    App::test((), |mut app| async move {
        let session = session_in_state(&mut app, true, true, false);

        let error = session
            .send(
                &mut app,
                json!({ "text": "rm -rf x", "expected_user_input_count": 0 }),
            )
            .expect_err("text must not land on a shell prompt");

        assert_eq!(error.code, ErrorCode::TargetStateConflict);
        assert!(error.message.contains("long command"));
        assert!(session.pty_writes.borrow().is_empty());
    });
}

#[test]
fn only_counted_writes_advance_the_user_input_count() {
    App::test((), |mut app| async move {
        let session = local_control_session(&mut app, true);
        assert_eq!(session.user_input_count(&app), 0);

        // Keyboard, paste and IME input reach the PTY through `write_user_bytes_to_pty`.
        session.terminal.update(&mut app, |view, ctx| {
            view.write_user_bytes_to_pty(b"typed".to_vec(), ctx);
        });
        assert_eq!(session.user_input_count(&app), 1);

        // Wheel and mouse translations, and `session.send_input` itself, pass `false`.
        session.terminal.update(&mut app, |view, ctx| {
            view.write_user_bytes_to_pty_inner(b"\x1bOA".to_vec(), false, ctx);
        });
        assert_eq!(session.user_input_count(&app), 1);
    });
}
