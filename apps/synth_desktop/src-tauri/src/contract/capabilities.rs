//! Public projections of domain-owned operations. Business logic lives with
//! the domain. This catalogue is intentionally partial during migration.

pub use crate::domains::visuals::operations::{ListVisualTemplates, ListVisualTemplatesRequest};

use schemars::{
    schema::{RootSchema, Schema},
    visit::{self, Visitor},
};
use serde_json::{json, Value};

/// Some MCP clients reject boolean schemas for unconstrained JSON values.
/// Project `true` as the equivalent `{}` without touching boolean defaults or
/// `additionalProperties: false`. The Rust wire types remain authoritative.
fn mcp_schema<T: schemars::JsonSchema>() -> RootSchema {
    struct ObjectSchemas;
    impl Visitor for ObjectSchemas {
        fn visit_schema_object(&mut self, schema: &mut schemars::schema::SchemaObject) {
            // Rust/OpenAPI numeric format names are not JSON Schema formats.
            // Keep their actual integer/number type and range constraints.
            if matches!(
                schema.format.as_deref(),
                Some("int32" | "int64" | "uint32" | "uint64" | "float" | "double")
            ) {
                schema.format = None;
            }
            visit::visit_schema_object(self, schema);
        }

        fn visit_schema(&mut self, schema: &mut Schema) {
            if matches!(schema, Schema::Bool(true)) {
                *schema = Schema::Object(Default::default());
            }
            visit::visit_schema(self, schema);
        }
    }
    let mut schema = schemars::schema_for!(T);
    ObjectSchemas.visit_root_schema(&mut schema);
    schema
}

/// Metadata projects one typed operation onto a protocol. Input/output schemas
/// are derived from its actual wire types; callers cannot edit a parallel copy.
pub trait PublicOperation {
    type Request: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema;
    type Response: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema;

    const ID: &'static str;
    const MCP_NAME: &'static str;
    const DESCRIPTION: &'static str;
    const READ_ONLY: bool;
    const DESTRUCTIVE: bool;
    const IDEMPOTENT: bool;

    fn mcp_tool() -> Value {
        json!({
            "name": Self::MCP_NAME,
            "description": Self::DESCRIPTION,
            "inputSchema": mcp_schema::<Self::Request>(),
            "outputSchema": mcp_schema::<Self::Response>(),
            "annotations": {
                "readOnlyHint": Self::READ_ONLY,
                "destructiveHint": Self::DESTRUCTIVE,
                "idempotentHint": Self::IDEMPOTENT,
                "openWorldHint": false
            },
            "_meta": {"workshop/operationId": Self::ID}
        })
    }
}

/// Migrated definitions only. This is not a claim of whole-product coverage.
pub fn migrated_tools() -> Vec<Value> {
    vec![ListVisualTemplates::mcp_tool()]
}

