//! Game coordinator shared objects.
//!
//! The game coordinator sends game state as shared objects. Every game has
//! its own set of shared object types.

use prost::Message as _;

use crate::nethook::body::{Body, ProtobufBody};
use crate::nethook::render::protobuf_node;
use crate::nethook::specializations::{
    ExtraObject, GameCoordinatorSpecialization, SpecializationContext,
};
use crate::nethook::well_known_app_ids as app;

/// The decoder of a shared object of a game.
type SharedObjectDecoder = fn(i32, &[u8]) -> Option<(String, Box<dyn ProtobufBody>)>;

macro_rules! shared_object_decoder {
    ($fn_name:ident, $($type_id:expr => $name:literal => $ty:ty),* $(,)?) => {
        /// Decodes the shared object with the given type identifier.
        fn $fn_name(type_id: i32, data: &[u8]) -> Option<(String, Box<dyn ProtobufBody>)> {
            match type_id {
                $(
                    $type_id => {
                        let message: Box<dyn ProtobufBody> =
                            Box::new(<$ty>::decode(data).ok()?);
                        Some(($name.to_owned(), message))
                    }
                )*
                _ => None,
            }
        }
    };
}

shared_object_decoder!(
    decode_csgo,
    1 => "CSOEconItem" => steamkit_protos::csgo::CsoEconItem,
    2 => "CSOPersonaDataPublic" => steamkit_protos::csgo::CsoPersonaDataPublic,
    5 => "CSOItemRecipe" => steamkit_protos::csgo::CsoItemRecipe,
    7 => "CSOEconGameAccountClient" => steamkit_protos::csgo::CsoEconGameAccountClient,
    38 => "CSOEconItemDropRateBonus" => steamkit_protos::csgo::CsoEconItemDropRateBonus,
    40 => "CSOAccountSeasonalOperation" => steamkit_protos::csgo::CsoAccountSeasonalOperation,
    41 => "CSOAccountSeasonalOperation" => steamkit_protos::csgo::CsoAccountSeasonalOperation,
    45 => "CSOEconCoupon" => steamkit_protos::csgo::CsoEconCoupon,
    46 => "CSOQuestProgress" => steamkit_protos::csgo::CsoQuestProgress,
);

shared_object_decoder!(
    decode_dota2,
    1 => "CSOEconItem" => steamkit_protos::dota2::CsoEconItem,
    7 => "CSOEconGameAccountClient" => steamkit_protos::dota2::CsoEconGameAccountClient,
    38 => "CSOEconItemDropRateBonus" => steamkit_protos::dota2::CsoEconItemDropRateBonus,
    42 => "CSOEconItemTournamentPassport" => steamkit_protos::dota2::CsoEconItemTournamentPassport,
    2002 => "CSODOTAGameAccountClient" => steamkit_protos::dota2::CsodotaGameAccountClient,
    2003 => "CSODOTAParty" => steamkit_protos::dota2::CsodotaParty,
    2004 => "CSODOTALobby" => steamkit_protos::dota2::CsodotaLobby,
    2006 => "CSODOTAPartyInvite" => steamkit_protos::dota2::CsodotaPartyInvite,
    2007 => "CSODOTAGameHeroFavorites" => steamkit_protos::dota2::CsodotaGameHeroFavorites,
    2008 => "CSODOTAMapLocationState" => steamkit_protos::dota2::CsodotaMapLocationState,
    2009 => "CMsgDOTATournament" => steamkit_protos::dota2::CMsgDotaTournament,
    2010 => "CSODOTAPlayerChallenge" => steamkit_protos::dota2::CsodotaPlayerChallenge,
    2011 => "CSODOTALobbyInvite" => steamkit_protos::dota2::CsodotaLobbyInvite,
    2012 => "CSODOTAGameAccountPlus" => steamkit_protos::dota2::CsodotaGameAccountPlus,
    2013 => "CSODOTALobby" => steamkit_protos::dota2::CsodotaLobby,
    2014 => "CSODOTAStaticLobby" => steamkit_protos::dota2::CsodotaStaticLobby,
    2015 => "CSODOTAServerLobby" => steamkit_protos::dota2::CsodotaServerLobby,
    2016 => "CSODOTAServerStaticLobby" => steamkit_protos::dota2::CsodotaServerStaticLobby,
);

shared_object_decoder!(
    decode_tf2,
    1 => "CSOEconItem" => steamkit_protos::tf2::CsoEconItem,
    2 => "CSOTFPlayerInfo" => steamkit_protos::tf2::CsotfPlayerInfo,
    3 => "CSOEconClaimCode" => steamkit_protos::tf2::CsoEconClaimCode,
    5 => "CSOItemRecipe" => steamkit_protos::tf2::CsoItemRecipe,
    7 => "CSOEconGameAccountClient" => steamkit_protos::tf2::CsoEconGameAccountClient,
    19 => "CSOTFDuelSummary" => steamkit_protos::tf2::CsotfDuelSummary,
    28 => "CSOTFMapContribution" => steamkit_protos::tf2::CsotfMapContribution,
    36 => "CSOClassPresetClientData" => steamkit_protos::tf2::CsoClassPresetClientData,
    37 => "CSOEconGameAccountForGameServers" => steamkit_protos::tf2::CsoEconGameAccountForGameServers,
    38 => "CSOWarData" => steamkit_protos::tf2::CsoWarData,
    39 => "CSOTFLadderPlayerStats" => steamkit_protos::tf2::CsotfLadderPlayerStats,
    40 => "CSOTFMatchResultPlayerStats" => steamkit_protos::tf2::CsotfMatchResultPlayerStats,
    41 => "CMsgTFXPSource" => steamkit_protos::tf2::CMsgTfxpSource,
    42 => "CMsgGCNotification" => steamkit_protos::tf2::CMsgGcNotification,
    2003 => "CSOTFParty" => steamkit_protos::tf2::CsotfParty,
    2004 => "CSOTFGameServerLobby" => steamkit_protos::tf2::CsotfGameServerLobby,
    2006 => "CSOTFPartyInvite" => steamkit_protos::tf2::CsotfPartyInvite,
    2007 => "CSOTFRatingData" => steamkit_protos::tf2::CsotfRatingData,
    2008 => "CTFLobbyInviteProto" => steamkit_protos::tf2::CtfLobbyInviteProto,
);

shared_object_decoder!(
    decode_deadlock,
    1 => "CSOEconItem" => steamkit_protos::deadlock::CsoEconItem,
    7 => "CSOEconGameAccountClient" => steamkit_protos::deadlock::CsoEconGameAccountClient,
    101 => "CSOCitadelLobby" => steamkit_protos::deadlock::CsoCitadelLobby,
    102 => "CSOCitadelServerStaticLobby" => steamkit_protos::deadlock::CsoCitadelServerStaticLobby,
    104 => "CSOGameAccountClient" => steamkit_protos::deadlock::CsoGameAccountClient,
    105 => "CSOCitadelParty" => steamkit_protos::deadlock::CsoCitadelParty,
    106 => "CSOCitadelServerDynamicLobby" => steamkit_protos::deadlock::CsoCitadelServerDynamicLobby,
    107 => "CSOAccountHeroInfo" => steamkit_protos::deadlock::CsoAccountHeroInfo,
    109 => "CSOAccountChallenge" => steamkit_protos::deadlock::CsoAccountChallenge,
    110 => "CSOCitadelHideoutLobby" => steamkit_protos::deadlock::CsoCitadelHideoutLobby,
);

/// Decodes the shared objects of Counter Strike 2.
pub struct CsgoSharedObjects;

impl GameCoordinatorSpecialization for CsgoSharedObjects {
    fn get_extra_objects(
        &self,
        body: &Body,
        app_id: u32,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        if app_id != app::COUNTER_STRIKE_2 {
            return Vec::new();
        }

        let mut objects = Vec::new();

        if let Some(single) = body.decode_as::<steamkit_protos::csgo::CMsgSoSingleObject>() {
            objects.extend(shared_objects(
                "SO",
                decode_csgo,
                single.type_id.unwrap_or_default(),
                single.object_data.as_deref().unwrap_or_default(),
                context,
            ));
        }

        if let Some(multiple) = body.decode_as::<steamkit_protos::csgo::CMsgSoMultipleObjects>() {
            for single in &multiple.objects_modified {
                objects.extend(shared_objects(
                    "Modified SO",
                    decode_csgo,
                    single.type_id.unwrap_or_default(),
                    single.object_data.as_deref().unwrap_or_default(),
                    context,
                ));
            }
        }

        if let Some(cache) = body.decode_as::<steamkit_protos::csgo::CMsgSoCacheSubscribed>() {
            for bucket in &cache.objects {
                for data in &bucket.object_data {
                    objects.extend(shared_objects(
                        "SO",
                        decode_csgo,
                        bucket.type_id.unwrap_or_default(),
                        data,
                        context,
                    ));
                }
            }
        }

        objects
    }
}

/// Decodes the shared objects of Dota 2.
pub struct Dota2SharedObjects;

impl GameCoordinatorSpecialization for Dota2SharedObjects {
    fn get_extra_objects(
        &self,
        body: &Body,
        app_id: u32,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        if app_id != app::DOTA_2 {
            return Vec::new();
        }

        let mut objects = Vec::new();

        if let Some(single) = body.decode_as::<steamkit_protos::dota2::CMsgSoSingleObject>() {
            objects.extend(shared_objects(
                "SO",
                decode_dota2,
                single.type_id.unwrap_or_default(),
                single.object_data.as_deref().unwrap_or_default(),
                context,
            ));
        }

        if let Some(multiple) = body.decode_as::<steamkit_protos::dota2::CMsgSoMultipleObjects>() {
            for single in &multiple.objects_added {
                objects.extend(shared_objects(
                    "New SO",
                    decode_dota2,
                    single.type_id.unwrap_or_default(),
                    single.object_data.as_deref().unwrap_or_default(),
                    context,
                ));
            }
            for single in &multiple.objects_modified {
                objects.extend(shared_objects(
                    "Modified SO",
                    decode_dota2,
                    single.type_id.unwrap_or_default(),
                    single.object_data.as_deref().unwrap_or_default(),
                    context,
                ));
            }
            for single in &multiple.objects_removed {
                objects.extend(shared_objects(
                    "Removed SO",
                    decode_dota2,
                    single.type_id.unwrap_or_default(),
                    single.object_data.as_deref().unwrap_or_default(),
                    context,
                ));
            }
        }

        if let Some(cache) = body.decode_as::<steamkit_protos::dota2::CMsgSoCacheSubscribed>() {
            for bucket in &cache.objects {
                for data in &bucket.object_data {
                    objects.extend(shared_objects(
                        "SO",
                        decode_dota2,
                        bucket.type_id.unwrap_or_default(),
                        data,
                        context,
                    ));
                }
            }
        }

        objects
    }
}

/// Decodes the shared objects of Team Fortress 2.
pub struct Tf2SharedObjects;

impl GameCoordinatorSpecialization for Tf2SharedObjects {
    fn get_extra_objects(
        &self,
        body: &Body,
        app_id: u32,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        if app_id != app::TEAM_FORTRESS_2 {
            return Vec::new();
        }

        let mut objects = Vec::new();

        if let Some(single) = body.decode_as::<steamkit_protos::tf2::CMsgSoSingleObject>() {
            objects.extend(shared_objects(
                "SO",
                decode_tf2,
                single.type_id.unwrap_or_default(),
                single.object_data.as_deref().unwrap_or_default(),
                context,
            ));
        }

        if let Some(multiple) = body.decode_as::<steamkit_protos::tf2::CMsgSoMultipleObjects>() {
            for single in &multiple.objects {
                objects.extend(shared_objects(
                    "SO",
                    decode_tf2,
                    single.type_id.unwrap_or_default(),
                    single.object_data.as_deref().unwrap_or_default(),
                    context,
                ));
            }
        }

        if let Some(cache) = body.decode_as::<steamkit_protos::tf2::CMsgSoCacheSubscribed>() {
            for bucket in &cache.objects {
                for data in &bucket.object_data {
                    objects.extend(shared_objects(
                        "SO",
                        decode_tf2,
                        bucket.type_id.unwrap_or_default(),
                        data,
                        context,
                    ));
                }
            }
        }

        objects
    }
}

/// Decodes the shared objects of Deadlock.
pub struct DeadlockSharedObjects;

impl GameCoordinatorSpecialization for DeadlockSharedObjects {
    fn get_extra_objects(
        &self,
        body: &Body,
        app_id: u32,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        if app_id != app::DEADLOCK {
            return Vec::new();
        }

        let mut objects = Vec::new();

        if let Some(single) = body.decode_as::<steamkit_protos::deadlock::CMsgSoSingleObject>() {
            objects.extend(shared_objects(
                "SO",
                decode_deadlock,
                single.type_id.unwrap_or_default(),
                single.object_data.as_deref().unwrap_or_default(),
                context,
            ));
        }

        if let Some(multiple) = body.decode_as::<steamkit_protos::deadlock::CMsgSoMultipleObjects>() {
            for single in &multiple.objects_added {
                objects.extend(shared_objects(
                    "New SO",
                    decode_deadlock,
                    single.type_id.unwrap_or_default(),
                    single.object_data.as_deref().unwrap_or_default(),
                    context,
                ));
            }
            for single in &multiple.objects_modified {
                objects.extend(shared_objects(
                    "Modified SO",
                    decode_deadlock,
                    single.type_id.unwrap_or_default(),
                    single.object_data.as_deref().unwrap_or_default(),
                    context,
                ));
            }
            for single in &multiple.objects_removed {
                objects.extend(shared_objects(
                    "Removed SO",
                    decode_deadlock,
                    single.type_id.unwrap_or_default(),
                    single.object_data.as_deref().unwrap_or_default(),
                    context,
                ));
            }
        }

        if let Some(cache) = body.decode_as::<steamkit_protos::deadlock::CMsgSoCacheSubscribed>() {
            for bucket in &cache.objects {
                for data in &bucket.object_data {
                    objects.extend(shared_objects(
                        "SO",
                        decode_deadlock,
                        bucket.type_id.unwrap_or_default(),
                        data,
                        context,
                    ));
                }
            }
        }

        objects
    }
}

/// Decodes a shared object of a game.
fn shared_objects(
    label: &str,
    decoder: SharedObjectDecoder,
    type_id: i32,
    data: &[u8],
    context: &SpecializationContext<'_>,
) -> Vec<ExtraObject> {
    let (type_name, message) = match decoder(type_id, data) {
        Some(decoded) => decoded,
        None => return Vec::new(),
    };

    let node = protobuf_node(&type_name, &*message, context.show_unset_fields);

    vec![ExtraObject::new(format!("{label} ({type_name})"), node)]
}
