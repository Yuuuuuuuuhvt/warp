use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ::local_control::protocol::{TabCreateParams, TabPlacement, TargetSelector};
use ::local_control::{ErrorCode, InstanceId};
use warpui::{App, TypedActionView, ViewHandle, WindowId};

use super::{create_tab, insert_tab_config_tab, select_tab_config};
use crate::local_control::LocalControlBridge;
use crate::local_control::resolver::{
    background_tab_config_window_id, decode_params, tab_create_window_id_for_target,
    validate_window_create_params,
};
use crate::tab_configs::tab_config::{TabConfigPaneNode, TabConfigPaneType};
use crate::tab_configs::{TabConfig, TabConfigParam, TabConfigParamType};
use crate::workspace::view::tests::{initialize_app, mock_workspace};
use crate::workspace::{Workspace, WorkspaceAction};

fn test_tab_config(name: &str) -> TabConfig {
    TabConfig {
        name: name.to_owned(),
        title: None,
        color: None,
        panes: vec![TabConfigPaneNode {
            id: "main".to_owned(),
            pane_type: Some(TabConfigPaneType::Terminal),
            split: None,
            children: None,
            is_focused: Some(true),
            directory: None,
            commands: None,
            shell: None,
        }],
        params: HashMap::new(),
        source_path: None,
    }
}

#[test]
fn tab_create_handler_adds_and_activates_terminal_tab() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let previous_count = workspace.read(&app, |workspace, _| workspace.tab_count());
        let bridge = app.add_singleton_model(LocalControlBridge::new);
        let instance_id = InstanceId("inst_test".to_owned());

        let response = bridge.update(&mut app, |bridge, ctx| {
            bridge.set_instance_id(instance_id.clone());
            create_tab(
                &Some(instance_id.clone()),
                &serde_json::json!({}),
                &TargetSelector::default(),
                ctx,
            )
            .expect("tab.create handler succeeds")
        });

        workspace.read(&app, |workspace, _| {
            assert_eq!(workspace.tab_count(), previous_count + 1);
            assert_eq!(workspace.active_tab_index(), previous_count);
        });
        assert_eq!(response["action"], "tab.create");
        assert_eq!(response["created"], true);
        assert_eq!(response["instance_id"], "inst_test");
        assert_eq!(response["tab"]["previous_count"], previous_count);
        assert_eq!(response["tab"]["count"], previous_count + 1);
        assert_eq!(response["tab"]["active_index"], previous_count);
        assert_eq!(response["tab"]["index"], previous_count);
        assert_eq!(response["tab"]["activated"], true);
        assert!(response["tab"]["id"].is_string());
        assert_eq!(
            response["tab"]["session_ids"].as_array().map(Vec::len),
            Some(1)
        );
        // Only the sessions of the tab this request created become writable by
        // session.send_input; tabs the user already had stay unmarked.
        assert_eq!(
            created_by_local_control_flags(&workspace, previous_count, &app),
            vec![true]
        );
        assert_eq!(
            created_by_local_control_flags(&workspace, 0, &app),
            vec![false]
        );
    });
}

/// The `created_by_local_control` marker of every terminal session in the tab at `index`.
fn created_by_local_control_flags(
    workspace: &ViewHandle<Workspace>,
    index: usize,
    app: &App,
) -> Vec<bool> {
    workspace.read(app, |workspace, ctx| {
        let pane_group = workspace.get_pane_group_view(index).unwrap();
        pane_group.read(ctx, |pane_group, ctx| {
            pane_group
                .visible_pane_ids()
                .into_iter()
                .filter_map(|pane_id| pane_group.terminal_view_from_pane_id(pane_id, ctx))
                .map(|terminal_view| {
                    terminal_view.read(ctx, |view, _| view.created_by_local_control())
                })
                .collect()
        })
    })
}

#[test]
fn tab_create_rejects_shell_parameter() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let _workspace = mock_workspace(&mut app);
        let bridge = app.add_singleton_model(LocalControlBridge::new);
        let instance_id = InstanceId("inst_test".to_owned());

        let err = bridge.update(&mut app, |bridge, ctx| {
            bridge.set_instance_id(instance_id.clone());
            create_tab(
                &Some(instance_id.clone()),
                &serde_json::json!({ "shell": "zsh" }),
                &TargetSelector::default(),
                ctx,
            )
            .expect_err("shell parameter must be rejected")
        });

        assert_eq!(err.code, ErrorCode::InvalidParams);
    });
}

#[test]
fn tab_create_rejects_tab_config_combined_with_tab_type() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let previous_count = workspace.read(&app, |workspace, _| workspace.tab_count());
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        let err = bridge.update(&mut app, |_, ctx| {
            create_tab(
                &None,
                &serde_json::json!({ "tab_config": "dev_setup", "tab_type": "terminal" }),
                &TargetSelector::default(),
                ctx,
            )
            .expect_err("tab_config and tab_type must not be combined")
        });

        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert!(err.message.contains("tab_config"));
        assert_eq!(
            workspace.read(&app, |workspace, _| workspace.tab_count()),
            previous_count
        );
    });
}

#[test]
fn tab_create_rejects_activation_options_without_tab_config() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let previous_count = workspace.read(&app, |workspace, _| workspace.tab_count());
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        for params in [
            serde_json::json!({ "activate": false }),
            serde_json::json!({ "placement": "after_all_tabs" }),
        ] {
            let err = bridge.update(&mut app, |_, ctx| {
                create_tab(&None, &params, &TargetSelector::default(), ctx)
                    .expect_err("activation options require tab_config")
            });
            assert_eq!(err.code, ErrorCode::InvalidParams);
            assert!(err.message.contains("tab_config"));
        }
        assert_eq!(
            workspace.read(&app, |workspace, _| workspace.tab_count()),
            previous_count
        );
    });
}

#[test]
fn tab_create_rejects_unsafe_tab_config_names_before_inserting() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let previous_count = workspace.read(&app, |workspace, _| workspace.tab_count());
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        for name in ["", ".", "..", "nested/dev_setup", "nested\\dev_setup"] {
            let err = bridge.update(&mut app, |_, ctx| {
                create_tab(
                    &None,
                    &serde_json::json!({ "tab_config": name }),
                    &TargetSelector::default(),
                    ctx,
                )
                .expect_err("a tab config name that is not a plain file name must be rejected")
            });
            assert_eq!(err.code, ErrorCode::InvalidParams);
        }

        assert_eq!(
            workspace.read(&app, |workspace, _| workspace.tab_count()),
            previous_count
        );
    });
}

#[test]
fn tab_create_background_fallback_reports_missing_target_without_a_workspace_window() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        let err = bridge.update(&mut app, |_, ctx| {
            tab_create_window_id_for_target(ctx, &TargetSelector::default(), true)
                .expect_err("no window hosts a workspace")
        });

        assert_eq!(err.code, ErrorCode::MissingTarget);
    });
}

#[test]
fn background_tab_config_window_prefers_front_to_back_order() {
    let older = WindowId::from_usize(1);
    let newer = WindowId::from_usize(2);
    let without_workspace = WindowId::from_usize(3);

    assert_eq!(
        background_tab_config_window_id(&[without_workspace, older, newer], &[older, newer]),
        Some(older)
    );
}

#[test]
fn background_tab_config_window_falls_back_to_workspace_windows_without_order() {
    let only = WindowId::from_usize(4);
    let older = WindowId::from_usize(1);
    let newer = WindowId::from_usize(2);
    let without_workspace = WindowId::from_usize(3);

    assert_eq!(background_tab_config_window_id(&[], &[only]), Some(only));
    assert_eq!(
        background_tab_config_window_id(&[without_workspace], &[newer, older]),
        Some(newer)
    );
    assert_eq!(
        background_tab_config_window_id(&[without_workspace], &[]),
        None
    );
}

/// The test platform reports no active window and no front-to-back order, which is the state of
/// a background-launched window on KDE. The tab config must still open in that window, and only
/// in the background.
#[test]
fn tab_create_background_fallback_opens_in_the_only_window_without_activating() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let window_id = app.read(|ctx| workspace.window_id(ctx));
        let active_before =
            workspace.read(&app, |workspace, _| workspace.active_tab_pane_group().id());
        let focused_before = app.read(|ctx| ctx.windows().last_window_shown_and_focused_for_test());
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        let resolved = bridge.update(&mut app, |_, ctx| {
            assert_eq!(ctx.windows().active_window(), None);
            assert!(ctx.windows().ordered_window_ids().is_empty());
            tab_create_window_id_for_target(ctx, &TargetSelector::default(), true)
                .expect("the only workspace window is the target")
        });
        assert_eq!(resolved, window_id);

        let params = TabCreateParams {
            tab_type: None,
            tab_config: Some("background".to_owned()),
            activate: Some(false),
            placement: Some(TabPlacement::AfterAllTabs),
        };
        let inserted = workspace.update(&mut app, |workspace, ctx| {
            insert_tab_config_tab(workspace, test_tab_config("background"), &params, ctx)
                .expect("background tab config insert succeeds")
        });
        let response = app.read(|ctx| inserted.into_response(ctx));

        assert!(!response.activated);
        workspace.read(&app, |workspace, _| {
            assert_eq!(workspace.active_tab_pane_group().id(), active_before);
        });
        app.read(|ctx| {
            assert_eq!(ctx.windows().active_window(), None);
            assert_eq!(
                ctx.windows().last_window_shown_and_focused_for_test(),
                focused_before
            );
        });
    });
}

#[test]
fn tab_create_background_fallback_picks_the_newest_of_several_windows() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let older = mock_workspace(&mut app);
        let newer = mock_workspace(&mut app);
        let newer_window_id = app.read(|ctx| newer.window_id(ctx));
        assert_ne!(app.read(|ctx| older.window_id(ctx)), newer_window_id);
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        let resolved = bridge.update(&mut app, |_, ctx| {
            tab_create_window_id_for_target(ctx, &TargetSelector::default(), true)
                .expect("the newest workspace window is the target")
        });

        assert_eq!(resolved, newer_window_id);
    });
}

#[test]
fn select_tab_config_rejects_unknown_names() {
    let dir = Path::new("/warp/tab_configs");
    let err = select_tab_config("dev_setup", Vec::new(), dir)
        .expect_err("an unknown tab config name must be rejected");

    assert_eq!(err.code, ErrorCode::InvalidParams);
    assert!(err.message.contains("dev_setup"));
    assert!(err.message.contains("/warp/tab_configs"));
}

#[test]
fn select_tab_config_rejects_configs_with_params() {
    let mut config = test_tab_config("needs_input");
    config.params.insert(
        "branch".to_owned(),
        TabConfigParam {
            description: None,
            default: None,
            param_type: TabConfigParamType::Text,
        },
    );

    let err = select_tab_config("needs_input", vec![config], Path::new("/warp/tab_configs"))
        .expect_err("a tab config that declares params must be rejected");

    assert_eq!(err.code, ErrorCode::InvalidParams);
    assert!(err.message.contains("needs_input"));
}

#[test]
fn select_tab_config_matches_by_file_stem() {
    let mut config = test_tab_config("dev_setup");
    config.source_path = Some(PathBuf::from("/warp/tab_configs/dev_setup.toml"));

    let selected = select_tab_config("DEV_SETUP", vec![config], Path::new("/warp/tab_configs"))
        .expect("a tab config matching the requested file stem is selected");

    assert_eq!(selected.name, "dev_setup");
}

#[test]
fn tab_create_inserts_tab_config_in_background_without_activating() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        // Three tabs with the first one active: the default AfterCurrentTab placement would
        // insert at index 1, so only an honoured AfterAllTabs placement yields index 3.
        workspace.update(&mut app, |workspace, ctx| {
            for _ in 0..2 {
                workspace.handle_action(
                    &WorkspaceAction::AddTerminalTab {
                        hide_homepage: false,
                    },
                    ctx,
                );
            }
            workspace.activate_tab(0, ctx);
        });
        let active_before =
            workspace.read(&app, |workspace, _| workspace.active_tab_pane_group().id());

        let params = TabCreateParams {
            tab_type: None,
            tab_config: Some("background".to_owned()),
            activate: Some(false),
            placement: Some(TabPlacement::AfterAllTabs),
        };
        let inserted = workspace.update(&mut app, |workspace, ctx| {
            insert_tab_config_tab(workspace, test_tab_config("background"), &params, ctx)
                .expect("background tab config insert succeeds")
        });
        let response = app.read(|ctx| inserted.into_response(ctx));

        assert_eq!(response.index, 3);
        assert!(!response.activated);
        assert_eq!(response.previous_count, 3);
        assert_eq!(response.count, 4);
        assert_eq!(response.active_index, 0);
        assert_ne!(response.id, active_before.to_string());
        assert_eq!(response.session_ids.len(), 1);
        workspace.read(&app, |workspace, _| {
            assert_eq!(workspace.active_tab_index(), 0);
            assert_eq!(workspace.active_tab_pane_group().id(), active_before);
            assert_eq!(
                workspace.get_pane_group_view(3).unwrap().id().to_string(),
                response.id
            );
        });
    });
}

#[test]
fn window_create_rejects_tab_config_fields() {
    for params in [
        serde_json::json!({ "tab_config": "dev_setup" }),
        serde_json::json!({ "activate": false }),
        serde_json::json!({ "placement": "after_all_tabs" }),
    ] {
        let params = decode_params::<TabCreateParams>(&params).expect("params decode");
        let err = validate_window_create_params(&params)
            .expect_err("window.create only accepts tab_type");

        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert!(err.message.contains("tab_type"));
    }
}
