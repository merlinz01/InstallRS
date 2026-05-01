//! The [`Component`] type — registered via [`crate::Installer::component`]
//! for optional wizard/CLI-selectable install features.

/// An optional feature the user can select or deselect at install time.
///
/// Registered via [`crate::Installer::component`]. The component's progress weight
/// contributes to the installer's step total whenever the component is
/// selected; operations performed while the component is active each advance
/// the cursor by their own weight (default 1).
///
/// ```rust,ignore
/// i.component("docs", "Documentation", "User-facing docs", 3);
/// i.component("extras", "Extras", "Optional samples", 1).default_off();
/// i.component("core", "Core files", "Always installed", 10).required();
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
    pub(crate) default: bool,
    pub(crate) required: bool,
    pub(crate) selected: bool,
}

impl Component {
    /// Mark this component as required: it cannot be unchecked, renders
    /// greyed-out in the wizard, and is always on in headless mode.
    pub fn required(&mut self) -> &mut Self {
        self.required = true;
        self.selected = true;
        self.default = true;
        self
    }

    /// Start this component unchecked. Components default to on — call this
    /// on ones the user has to opt into.
    pub fn default_off(&mut self) -> &mut Self {
        self.default = false;
        if !self.required {
            self.selected = false;
        }
        self
    }
}
