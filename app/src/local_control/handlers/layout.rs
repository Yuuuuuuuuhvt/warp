//! Layout mutation handlers for local-control actions.
#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
use std::path::Path;

use ::local_control::protocol::{TabCreateParams, TabPlacement, TabType, TargetSelector};
use ::local_control::{ActionKind, ControlError, ErrorCode, InstanceId};
use serde::Serialize;
use warpui::{AppContext, ModelContext, TypedActionView, ViewContext, ViewHandle};

use crate::local_control::LocalControlBridge;
use crate::local_control::resolver::{
    decode_params, tab_create_window_id_for_target, validate_tab_create_params,
    validate_tab_create_target, workspace_for_window,
};
use crate::pane_group::PaneGroup;
use crate::tab_configs::TabConfig;
use crate::uri::find_matching_tab_config;
use crate::user_config::{load_tab_configs, tab_configs_dir};
use crate::workspace::tab_settings::NewTabPlacement;
use crate::workspace::{TabInsertOptions, Workspace, WorkspaceAction};

#[derive(Serialize)]
struct TabCreateResponse<'a> {
    action: &'static str,
    created: bool,
    instance_id: Option<&'a str>,
    window: TargetWindowResponse,
    tab: TabResponse,
}

#[derive(Serialize)]
struct TargetWindowResponse {
    selector: &'static str,
    id: String,
}

#[derive(Serialize)]
struct TabResponse {
    id: String,
    index: usize,
    activated: bool,
    session_ids: Vec<String>,
    previous_count: usize,
    count: usize,
    active_index: usize,
}

/// The tab a `tab.create` request inserted, read from the workspace after insertion.
struct InsertedTab {
    pane_group: ViewHandle<PaneGroup>,
    index: usize,
    previous_count: usize,
    count: usize,
    active_index: usize,
}

impl InsertedTab {
    fn into_response(self, ctx: &AppContext) -> TabResponse {
        TabResponse {
            id: self.pane_group.id().to_string(),
            index: self.index,
            activated: self.index == self.active_index,
            session_ids: session_ids_for_pane_group(&self.pane_group, ctx),
            previous_count: self.previous_count,
            count: self.count,
            active_index: self.active_index,
        }
    }
}

pub(crate) fn create_tab(
    instance_id: &Option<InstanceId>,
    params: &serde_json::Value,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    validate_tab_create_target(target)?;
    let params = decode_params::<TabCreateParams>(params)?;
    validate_tab_create_params(&params)?;
    let tab_config = params
        .tab_config
        .as_deref()
        .map(tab_config_by_name)
        .transpose()?;
    let window_id = tab_create_window_id_for_target(ctx, target, tab_config.is_some())?;
    let workspace = workspace_for_window(window_id, ActionKind::TabCreate, ctx)?;
    let inserted = workspace.update(ctx, |workspace, ctx| match tab_config {
        Some(tab_config) => insert_tab_config_tab(workspace, tab_config, &params, ctx),
        None => insert_typed_tab(workspace, &params, ctx),
    })?;
    let tab = inserted.into_response(ctx);
    serde_json::to_value(TabCreateResponse {
        action: ActionKind::TabCreate.as_str(),
        created: true,
        instance_id: instance_id.as_ref().map(|id| id.0.as_str()),
        window: TargetWindowResponse {
            selector: "target",
            id: window_id.to_string(),
        },
        tab,
    })
    .map_err(|err| {
        ControlError::with_details(
            ErrorCode::Internal,
            "failed to serialize local-control tab.create response",
            err.to_string(),
        )
    })
}

/// Opens `config` with the activation and placement the request asked for. The tab is inserted
/// without switching to it when `activate` is false, so the caller keeps its active tab.
fn insert_tab_config_tab(
    workspace: &mut Workspace,
    config: TabConfig,
    params: &TabCreateParams,
    ctx: &mut ViewContext<Workspace>,
) -> Result<InsertedTab, ControlError> {
    let name = config.name.clone();
    let options = TabInsertOptions {
        activate: params.activate.unwrap_or(true),
        placement: params.placement.map(new_tab_placement),
    };
    let previous_count = workspace.tab_count();
    let index = workspace
        .open_tab_config_with_options(config, options, ctx)
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::Internal,
                format!("tab.create could not open tab config '{name}'"),
            )
        })?;
    inserted_tab(workspace, index, previous_count)
}

fn insert_typed_tab(
    workspace: &mut Workspace,
    params: &TabCreateParams,
    ctx: &mut ViewContext<Workspace>,
) -> Result<InsertedTab, ControlError> {
    let action = tab_create_action(params)?;
    let previous_count = workspace.tab_count();
    workspace.handle_action(&action, ctx);
    inserted_tab(workspace, workspace.active_tab_index(), previous_count)
}

/// Reads back the tab at `index` together with the tab counts the response reports.
fn inserted_tab(
    workspace: &Workspace,
    index: usize,
    previous_count: usize,
) -> Result<InsertedTab, ControlError> {
    let pane_group = workspace
        .get_pane_group_view(index)
        .cloned()
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::Internal,
                "tab.create did not produce a tab identifier",
            )
        })?;
    Ok(InsertedTab {
        pane_group,
        index,
        previous_count,
        count: workspace.tab_count(),
        active_index: workspace.active_tab_index(),
    })
}

fn tab_create_action(params: &TabCreateParams) -> Result<WorkspaceAction, ControlError> {
    match params.tab_type {
        None | Some(TabType::Terminal) => Ok(WorkspaceAction::AddTerminalTab {
            hide_homepage: false,
        }),
        Some(TabType::Agent) => Ok(WorkspaceAction::AddAgentTab),
        Some(TabType::Default) => Ok(WorkspaceAction::AddDefaultTab),
        Some(TabType::CloudAgent) => Err(ControlError::new(
            ErrorCode::UnsupportedAction,
            "tab.create does not support cloud-agent tabs",
        )),
    }
}

/// Loads the tab config whose file stem matches `name`.
fn tab_config_by_name(name: &str) -> Result<TabConfig, ControlError> {
    let dir = tab_configs_dir();
    let (configs, _errors) = load_tab_configs(&dir);
    select_tab_config(name, configs, &dir)
}

/// Selects a loaded tab config by name. A config that declares params is rejected, because
/// opening it would show the param-fill modal and take focus away from the active tab.
fn select_tab_config(
    name: &str,
    configs: Vec<TabConfig>,
    dir: &Path,
) -> Result<TabConfig, ControlError> {
    let config = find_matching_tab_config(name, configs).ok_or_else(|| {
        ControlError::new(
            ErrorCode::InvalidParams,
            format!("no tab config named '{name}' in {}", dir.display()),
        )
    })?;
    if !config.params.is_empty() {
        return Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!("tab config '{name}' declares params, which would open the param-fill modal"),
        ));
    }
    Ok(config)
}

fn new_tab_placement(placement: TabPlacement) -> NewTabPlacement {
    match placement {
        TabPlacement::AfterCurrentTab => NewTabPlacement::AfterCurrentTab,
        TabPlacement::AfterAllTabs => NewTabPlacement::AfterAllTabs,
    }
}

/// Session identifiers of the terminal panes in `pane_group`, in the same format as
/// `session.list` reports them.
fn session_ids_for_pane_group(pane_group: &ViewHandle<PaneGroup>, ctx: &AppContext) -> Vec<String> {
    pane_group.read(ctx, |pane_group, ctx| {
        pane_group
            .visible_pane_ids()
            .into_iter()
            .filter(|pane_id| {
                pane_group
                    .terminal_view_from_pane_id(*pane_id, ctx)
                    .is_some()
            })
            .map(|pane_id| pane_id.to_string())
            .collect()
    })
}
