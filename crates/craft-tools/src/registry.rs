//! The tool registry: builtins, cached wire specs, and the
//! contract-wrapped execute path the runtime drives.

use std::collections::HashMap;

use serde_json::Value;

use crate::bash::BashTool;
use crate::edit::EditFileTool;
use crate::error::ToolError;
use crate::event::{self, ToolStream};
use crate::gh::GhTool;
use crate::glob::GlobTool;
use crate::grep::GrepTool;
use crate::read::ReadFileTool;
use crate::tool::{Tool, ToolCtx, ToolLimits};
use crate::write::WriteFileTool;

/// One tool's wire-facing declaration: name, description, and the JSON
/// schema for its arguments. Built once at registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

pub struct Registry {
    tools: HashMap<String, Box<dyn Tool>>,
    specs: HashMap<String, ToolSpec>,
    limits: ToolLimits,
}

impl Registry {
    pub fn new(limits: ToolLimits) -> Self {
        Self {
            tools: HashMap::new(),
            specs: HashMap::new(),
            limits,
        }
    }

    /// Register a tool; its wire spec is computed once and cached.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        let spec = ToolSpec {
            name: name.clone(),
            description: tool.description().to_string(),
            schema: tool.params_schema().to_value(),
        };
        self.specs.insert(name.clone(), spec);
        self.tools.insert(name, tool);
    }

    /// The full builtin vocabulary under the given limits.
    pub fn with_builtins(limits: ToolLimits) -> Self {
        let mut registry = Self::new(limits);
        registry.register(Box::new(BashTool::new(limits)));
        registry.register(Box::new(ReadFileTool::new(limits)));
        registry.register(Box::new(WriteFileTool::new(limits)));
        registry.register(Box::new(EditFileTool::new(limits)));
        registry.register(Box::new(GlobTool::new(limits)));
        registry.register(Box::new(GrepTool::new(limits)));
        registry.register(Box::new(GhTool::new(limits)));
        registry
    }

    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tools.keys().map(|k| k.as_str()).collect();
        names.sort_unstable();
        names
    }

    pub fn spec(&self, name: &str) -> Option<&ToolSpec> {
        self.specs.get(name)
    }

    pub fn limits(&self) -> ToolLimits {
        self.limits
    }

    /// Execute a tool by name with the progress/terminal contract
    /// enforced; unknown names surface as a typed error in the stream.
    pub fn execute(&self, name: &str, args: Value, ctx: &ToolCtx) -> ToolStream {
        match self.tools.get(name) {
            Some(tool) => event::enforce_contract(tool.execute(args, ctx)),
            None => event::error_stream(ToolError::UnknownTool {
                name: name.to_string(),
            }),
        }
    }
}
