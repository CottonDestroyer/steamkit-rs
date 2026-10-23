//! Specializations add extra decoded objects to a message's tree, for
//! example the inner message of a multi packet or game coordinator shared
//! objects.

pub mod client_service_method;
pub mod gc_generic;
pub mod mms_lobby;
pub mod multi_packet;
pub mod remote_client_steam;
pub mod shared_objects;

use crate::nethook::body::Body;
use crate::nethook::dump::NetHookDump;
use crate::nethook::render::TreeNode;

/// An object that a specialization found inside a message.
pub struct ExtraObject {
    /// The label of the object.
    pub name: String,
    /// The body of the object, when the specializations may look at it again.
    pub body: Option<Body>,
    /// The node that shows the object.
    pub node: TreeNode,
}

impl ExtraObject {
    /// Creates an object with a label and a node.
    pub fn new(name: impl Into<String>, node: TreeNode) -> Self {
        ExtraObject {
            name: name.into(),
            body: None,
            node,
        }
    }
}

/// The context a specialization runs in.
pub struct SpecializationContext<'a> {
    /// The dump the message belongs to.
    pub dump: &'a NetHookDump,
    /// Whether fields that were not set on the wire are shown.
    pub show_unset_fields: bool,
}

/// A specialization adds objects to the tree of a decoded message.
pub trait Specialization {
    /// The objects a message body contributes.
    fn read_extra_objects(
        &self,
        body: &Body,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject>;
}

/// A specialization of the game coordinator messages of one game.
pub trait GameCoordinatorSpecialization {
    /// The objects a game coordinator message contributes.
    fn get_extra_objects(
        &self,
        body: &Body,
        app_id: u32,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject>;
}

/// Every built-in specialization.
pub fn default_specializations() -> Vec<Box<dyn Specialization>> {
    vec![
        Box::new(client_service_method::ClientServiceMethodSpecialization),
        Box::new(client_service_method::ClientServiceMethodResponseSpecialization),
        Box::new(remote_client_steam::RemoteClientSteamToSteamSpecialization),
        Box::new(remote_client_steam::RemoteClientSteamBroadcastSpecialization),
        Box::new(multi_packet::MultiPacketSpecialization),
        Box::new(mms_lobby::MmsLobbyMessageSpecialization),
        Box::new(gc_generic::GcGenericSpecialization {
            game_coordinator_specializations: vec![
                Box::new(shared_objects::CsgoSharedObjects),
                Box::new(shared_objects::Dota2SharedObjects),
                Box::new(shared_objects::Tf2SharedObjects),
                Box::new(shared_objects::DeadlockSharedObjects),
            ],
        }),
    ]
}
