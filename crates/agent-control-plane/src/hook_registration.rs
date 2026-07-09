pub use crate::hooks::registration::{
    HookConfigStyle, HookRegistrationChange, LOOPER_HOOK_MARKER, is_owned_hook_command,
    normalize_hook_command, owned_hook_state_keys_for_hooks_path, register_owned_hooks_for_spec,
    unregister_owned_hooks_for_spec,
};

use std::path::Path;

use anyhow::Result;

use crate::hooks::adapter::{CodexHookAdapter, Homes, HookAdapter};

pub fn register_owned_hooks(
    codex_home: &Path,
    hook_command: &str,
) -> Result<HookRegistrationChange> {
    register_owned_hooks_for_spec(
        &CodexHookAdapter.spec(&Homes::for_codex_home(codex_home)),
        hook_command,
    )
}

pub fn unregister_owned_hooks(codex_home: &Path) -> Result<usize> {
    unregister_owned_hooks_for_spec(&CodexHookAdapter.spec(&Homes::for_codex_home(codex_home)))
}
