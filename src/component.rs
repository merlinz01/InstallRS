//! The [`Component`] type — registered via [`crate::Installer::add_component`]
//! for optional wizard/CLI-selectable install features.

/// An optional feature the user can select or deselect at install time.
///
/// Registered via [`crate::Installer::add_component`]. The component's progress weight
/// contributes to the installer's step total whenever the component is
/// selected; operations performed while the component is active each advance
/// the cursor by their own weight (default 1).
///
/// ```rust,ignore
/// i.add_component("docs", "Documentation", "User-facing docs", 3);
/// i.add_component("core", "Core files", "Always installed", 10).required();
/// // Opt-in component: register, then deselect.
/// i.add_component("extras", "Extras", "Optional samples", 1);
/// i.set_component_selected("extras", false);
/// ```
///
/// Query with [`crate::Installer::is_component_selected`] inside the install
/// callback to branch on user choice.
#[derive(Clone, Debug)]
pub struct Component {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) description: String,
    /// Step weight this component contributes to the global progress total
    /// when selected. Over/undershoot is possible if the actual op count
    /// diverges from this number.
    pub(crate) progress_weight: u32,
    pub(crate) required: bool,
    pub(crate) selected: bool,
}

impl Component {
    /// Mark this component as required: it cannot be unchecked, renders
    /// greyed-out in the wizard, and is always on in headless mode.
    pub fn required(&mut self) -> &mut Self {
        self.required = true;
        self.selected = true;
        self
    }
}
