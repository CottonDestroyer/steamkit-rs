//! Legacy unified messaging requests and responses, which carry the service
//! method name and the serialised method body.

use steamkit_protos::steam::{CMsgClientServiceMethodLegacy, CMsgClientServiceMethodLegacyResponse};

use crate::nethook::body::Body;
use crate::nethook::proto_registry;
use crate::nethook::render::protobuf_node;
use crate::nethook::specializations::{ExtraObject, Specialization, SpecializationContext};

/// Decodes the body of a legacy unified messaging request.
pub struct ClientServiceMethodSpecialization;

impl Specialization for ClientServiceMethodSpecialization {
    fn read_extra_objects(
        &self,
        body: &Body,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        let legacy = match body.decode_as::<CMsgClientServiceMethodLegacy>() {
            Some(legacy) => legacy,
            None => return Vec::new(),
        };

        let name = legacy.method_name.clone().unwrap_or_default();
        let serialized = legacy.serialized_method.clone().unwrap_or_default();

        let types = match proto_registry::service_method_types(&name) {
            Some(types) => types,
            None => return Vec::new(),
        };

        let message = match (types.request_decoder)(&serialized) {
            Ok(message) => message,
            Err(_) => return Vec::new(),
        };

        let node = protobuf_node(types.request_type, &*message, context.show_unset_fields);

        vec![ExtraObject::new("Service Method", node)]
    }
}

/// Decodes the body of a legacy unified messaging response.
pub struct ClientServiceMethodResponseSpecialization;

impl Specialization for ClientServiceMethodResponseSpecialization {
    fn read_extra_objects(
        &self,
        body: &Body,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        let legacy = match body.decode_as::<CMsgClientServiceMethodLegacyResponse>() {
            Some(legacy) => legacy,
            None => return Vec::new(),
        };

        let name = legacy.method_name.clone().unwrap_or_default();
        let serialized = legacy.serialized_method_response.clone().unwrap_or_default();

        let types = match proto_registry::service_method_types(&name) {
            Some(types) => types,
            None => {
                return vec![ExtraObject::new(
                    "Service Method Response",
                    crate::nethook::render::TreeNode::new("<null>", None),
                )]
            }
        };

        let message = match (types.response_decoder)(&serialized) {
            Ok(message) => message,
            Err(_) => {
                return vec![ExtraObject::new(
                    "Service Method Response",
                    crate::nethook::render::TreeNode::new("<null>", None),
                )]
            }
        };

        let node = protobuf_node(types.response_type, &*message, context.show_unset_fields);

        vec![ExtraObject::new("Service Method Response", node)]
    }
}
