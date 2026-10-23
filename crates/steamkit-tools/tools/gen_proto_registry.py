#!/usr/bin/env python3
"""Builds the nethook analyzer protobuf type registry from the Steam protos.

Reads:
  * crates/steamkit-protos/proto/**.proto          (canonical Valve message names)
  * target/debug/build/steamkit-protos-*/out/*.rs   (the prost Rust type names)
  * crates/steamkit/src/base/generated/steam_language.rs (the EMsg enum)

Writes:
  crates/steamkit-tools/src/nethook/proto_registry.rs
"""

import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
PROTO_ROOT = REPO / "crates" / "steamkit-protos" / "proto"
TARGET = REPO / "target" / "debug" / "build"
OUT = REPO / "crates" / "steamkit-tools" / "src" / "nethook" / "proto_registry.rs"

MODULES = ["steam", "gc", "csgo", "dota2", "tf2", "deadlock", "webui"]
GC_MODULES = ["tf2", "csgo", "dota2", "deadlock"]


def find_out_dir():
    best = None
    best_mtime = 0
    for d in TARGET.glob("steamkit-protos-*/out"):
        f = d / "steam" / "steam.rs"
        if f.is_file():
            m = f.stat().st_mtime
            if m > best_mtime:
                best_mtime = m
                best = d
    if best is None:
        raise SystemExit("no generated steamkit-protos output found")
    return best


def norm(name):
    return re.sub(r"[^0-9a-z]", "", name.lower())


def strip_comments(text):
    text = re.sub(r"//[^\n]*", "", text)
    return re.sub(r"/\*.*?\*/", "", text, flags=re.S)


def proto_message_names(path):
    """Top level message names of a .proto file, in declaration order."""
    text = strip_comments(path.read_text(encoding="utf-8", errors="replace"))
    names = []
    depth = 0
    for line in text.split("\n"):
        m = re.match(r"^\s*message\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{", line)
        if m:
            if depth == 0:
                names.append(m.group(1))
            depth += 1
            continue
        if depth > 0:
            depth += line.count("{") - line.count("}")
    return names


SERVICE_RE = re.compile(r"^\s*service\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{", re.M)
RPC_RE = re.compile(
    r"^\s*rpc\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(\s*\.?([A-Za-z_][A-Za-z0-9_]*)\s*\)"
    r"\s*returns\s*\(\s*\.?([A-Za-z_][A-Za-z0-9_]*)\s*\)",
    re.M,
)


def proto_services(path):
    """(service, method, request type, response type) tuples of a .proto file."""
    text = strip_comments(path.read_text(encoding="utf-8", errors="replace"))
    out = []
    for sm in SERVICE_RE.finditer(text):
        service = sm.group(1)
        depth = 1
        end = sm.end()
        for i in range(sm.end(), len(text)):
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
                if depth == 0:
                    end = i
                    break
        body = text[sm.end():end]
        for rm in RPC_RE.finditer(body):
            out.append((service, rm.group(1), rm.group(2), rm.group(3)))
    return out


def manifest():
    lines = (PROTO_ROOT / "manifest.txt").read_text(encoding="utf-8").splitlines()
    result = {}
    for line in lines:
        line = line.strip()
        if not line:
            continue
        parts = line.split()
        result[parts[0]] = parts[1:]
    return result


STRUCT_RE = re.compile(r"^pub struct ([A-Za-z_][A-Za-z0-9_]*)\s*\{", re.M)
ENUM_RE = re.compile(r"^pub enum ([A-Za-z_][A-Za-z0-9_]*)\s*\{", re.M)
VARIANT_RE = re.compile(r"^\s{4}([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(-?\d+)\s*,", re.M)
STR_NAME_RE = re.compile(r'Self::([A-Za-z_][A-Za-z0-9_]*)\s*=>\s*"([^"]*)"')


def parse_generated(path):
    text = path.read_text(encoding="utf-8", errors="replace")
    structs = [m.group(1) for m in STRUCT_RE.finditer(text)]
    enums = {}
    for m in ENUM_RE.finditer(text):
        name = m.group(1)
        start = m.end()
        end = text.find("\n}", start)
        body = text[start:end if end != -1 else len(text)]
        variants = {}
        for v in VARIANT_RE.finditer(body):
            variants[v.group(1)] = int(v.group(2))
        # the original protobuf spelling of every variant
        original = {}
        impl_start = text.find("impl " + name + " {", end)
        if impl_start != -1:
            impl_end = text.find("\n}", impl_start)
            impl_body = text[impl_start:impl_end if impl_end != -1 else len(text)]
            for s in STR_NAME_RE.finditer(impl_body):
                original[s.group(1)] = s.group(2)
        enums[name] = {"values": variants, "names": original}
    return structs, enums


EMSG_RE = re.compile(r"^pub enum EMsg\s*\{(.*?)^\}", re.M | re.S)


def parse_emsg(path):
    text = path.read_text(encoding="utf-8")
    m = EMSG_RE.search(text)
    if not m:
        raise SystemExit("EMsg enum not found")
    variants = []
    for line in m.group(1).split("\n"):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        vm = re.match(r"^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(-?\d+)\s*,$", line)
        if vm:
            variants.append((vm.group(1), int(vm.group(2))))
    return variants


def gc_type_suffix(gc_name):
    """Reduces a GC message name to the type name suffix to search for.

    `lstrip("GC")` strips every leading `G` and `C`, so `k_EMsgGCClientHello`
    becomes `lientHello`. That over-strips, but the suffix still matches
    `CMsgClientHello`.
    """
    s = gc_name
    s = s.replace("k_", "")
    s = s.replace("ESOMsg", "")
    s = s.lstrip("_")
    s = s.replace("EMsg", "")
    s = s.lstrip("GC")
    return s


def main():
    out_dir = find_out_dir()
    man = manifest()

    proto_names = {}
    for module in MODULES:
        names = []
        for proto in man.get(module, []):
            path = PROTO_ROOT / module / proto
            if not path.is_file():
                raise SystemExit(f"missing proto {path}")
            names.extend(proto_message_names(path))
        proto_names[module] = names

    rust_structs = {}
    rust_enums = {}
    for module in MODULES:
        path = out_dir / module / f"{module}.rs"
        if not path.is_file():
            rust_structs[module] = []
            rust_enums[module] = {}
            continue
        s, e = parse_generated(path)
        rust_structs[module] = s
        rust_enums[module] = e

    mapping = {}
    unresolved = []
    ambiguous = []
    for module in MODULES:
        by_norm = {}
        for rname in rust_structs[module]:
            by_norm.setdefault(norm(rname), []).append(rname)
        table = {}
        for pname in proto_names[module]:
            cands = by_norm.get(norm(pname), [])
            if len(cands) == 1:
                table[pname] = cands[0]
            elif len(cands) > 1:
                ambiguous.append((module, pname, cands))
                table[pname] = cands[0]
            else:
                unresolved.append((module, pname))
        mapping[module] = table

    print(f"proto->rust: {sum(len(v) for v in mapping.values())} mapped, "
          f"{len(unresolved)} unresolved, {len(ambiguous)} ambiguous")
    for item in unresolved[:20]:
        print("  unresolved:", item)

    emsg_variants = parse_emsg(REPO / "crates" / "steamkit" / "src" / "base" /
                               "generated" / "steam_language.rs")
    print(f"EMsg: {len(emsg_variants)} variants")

    overrides = {
        "ClientLogonGameServer": "CMsgClientLogon",
        "ClientGamesPlayed": "CMsgClientGamesPlayed",
        "ClientGamesPlayedNoDataBlob": "CMsgClientGamesPlayed",
        "ClientGamesPlayedWithDataBlob": "CMsgClientGamesPlayed",
        "ClientToGC": "CMsgGCClient",
        "ClientFromGC": "CMsgGCClient",
        "ClientFriendMsgIncoming": "CMsgClientFriendMsgIncoming",
        "ClientFriendMsgEchoToSender": "CMsgClientFriendMsgIncoming",
        "ClientCurrentUIMode": "CMsgClientUIMode",
        "ClientGetNumberOfCurrentPlayersDP": "CMsgDPGetNumberOfCurrentPlayers",
        "ClientGetNumberOfCurrentPlayersDPResponse": "CMsgDPGetNumberOfCurrentPlayersResponse",
        "AMGameServerUpdate": "CMsgGameServerData",
        "ClientDPUpdateAppJobReport": "CDataPublisher_ClientUpdateAppJob_Notification",
        "ClientPlayingSessionState": "CMsgClientPlayingSessionState",
        "ClientNetworkingCertRequestResponse": "CMsgClientNetworkingCertReply",
    }
    for emsg, pname in overrides.items():
        if pname not in mapping["steam"]:
            raise SystemExit(f"override {emsg} -> {pname} not in steam module")

    ordered = [mapping["steam"][p] for p in proto_names["steam"] if p in mapping["steam"]]
    rust_to_proto = {}
    for pname, rname in mapping["steam"].items():
        rust_to_proto.setdefault(rname, pname)

    emsg_map = {}
    multi = {}
    for name, _value in emsg_variants:
        if name in overrides:
            emsg_map[name] = mapping["steam"][overrides[name]]
            continue
        matches = [r for r in ordered if r.lower().endswith(name.lower())]
        if not matches:
            continue
        if len(matches) > 1:
            multi.setdefault(name, []).extend(matches)
        emsg_map[name] = matches[0]

    print(f"EMsg->proto: {len(emsg_map)} mapped")
    for name, matches in sorted(multi.items()):
        print(f"  ambiguous EMsg {name}: {matches}")

    # --- services -------------------------------------------------------
    services = []
    seen_services = set()
    for module in MODULES:
        for proto in man.get(module, []):
            for service, method, req, resp in proto_services(PROTO_ROOT / module / proto):
                if req not in mapping[module] or resp not in mapping[module]:
                    continue
                # If several modules declare the same method, the first
                # one (in MODULES order) wins.
                if (service, method) in seen_services:
                    continue
                seen_services.add((service, method))
                services.append((module, service, method,
                                 mapping[module][req], mapping[module][resp]))
    print(f"services: {len(services)}")

    # --- GC candidate suffixes ------------------------------------------
    gc_enum_names = {
        "tf2": ["EtfgcMsg", "EgcBaseMsg", "EsoMsg", "EgcSystemMsg", "EgcItemMsg", "EgcBaseClientMsg"],
        "dota2": ["EdotagcMsg", "EgcBaseMsg", "EsoMsg", "EgcItemMsg", "EgcBaseClientMsg"],
        "csgo": ["ECsgoGcMsg", "EgcBaseMsg", "EsoMsg", "EgcSystemMsg", "EgcItemMsg", "EgcBaseClientMsg"],
        "deadlock": ["EgcCitadelClientMessages", "EgcBaseMsg", "EsoMsg", "EgcItemMsg", "EgcBaseClientMsg"],
    }
    suffixes = set()
    for module, enums in gc_enum_names.items():
        for ename in enums:
            info = rust_enums[module].get(ename)
            if not info:
                print(f"  missing gc enum {module}.{ename}")
                continue
            for variant, original in info["names"].items():
                suffixes.add(gc_type_suffix(original))
    suffixes.discard("")
    print(f"gc name suffixes: {len(suffixes)}")

    # Types that are always included, even if no message name ends with them.
    GC_OVERRIDE_TYPES = {
        "tf2": ["CMsgClientHello", "CMsgClientWelcome", "CMsgSOSingleObject",
                "CMsgSOMultipleObjects"],
        "csgo": ["CMsgClientHello", "CMsgClientWelcome", "CMsgConnectionStatus",
                 "CMsgSOSingleObject", "CMsgSOMultipleObjects"],
        "dota2": ["CMsgClientHello", "CMsgClientWelcome", "CMsgConnectionStatus",
                  "CMsgSOSingleObject", "CMsgSOMultipleObjects",
                  "CMsgDOTATeamsInfo", "CMsgDOTAMyTeamInfoRequest"],
        "deadlock": ["CMsgClientHello", "CMsgClientWelcome", "CMsgConnectionStatus",
                     "CMsgSOSingleObject", "CMsgSOMultipleObjects"],
    }

    gc_tables = {}
    for module in GC_MODULES:
        entries = []
        for pname in proto_names[module]:
            rname = mapping[module].get(pname)
            if rname is None or not rname.startswith("CMsg"):
                continue
            if not any(rname.lower().endswith(s.lower()) for s in suffixes):
                if pname not in GC_OVERRIDE_TYPES.get(module, []):
                    continue
            entries.append((pname, rname))
        gc_tables[module] = entries
        print(f"gc {module}: {len(entries)} candidate types")

    # The shared GC messages live in gcsdk_gcmessages.proto and
    # base_gcmessages.proto. Every game has its own copy; use tf2's.
    base_files = {"gcsdk_gcmessages.proto", "base_gcmessages.proto"}
    base_names = set()
    for proto in man["tf2"]:
        if proto not in base_files:
            continue
        for pname in proto_message_names(PROTO_ROOT / "tf2" / proto):
            base_names.add(pname)
    tf2_decoders = dict(gc_tables["tf2"])
    base_entries = [(pname, tf2_decoders[pname]) for pname in proto_names["tf2"]
                    if pname in base_names and pname in tf2_decoders]
    print(f"gc base: {len(base_entries)} candidate types")

    # --- write the Rust registry ----------------------------------------
    lines = []
    w = lines.append
    w("//! Generated by `tools/gen_proto_registry.py` - do not edit by hand.")
    w("//!")
    w("//! Lookup tables that map message numbers, service methods and game")
    w("//! coordinator message names to their protobuf types, so the analyzer can")
    w("//! decode message bodies.")
    w("")
    w("use prost::Message as _;")
    w("")
    w("use steamkit::base::generated::steam_language::EMsg;")
    w("")
    w("use super::body::ProtobufBody;")
    w("")
    w("/// Decodes a protobuf message body from a byte slice.")
    w("pub type ProtobufDecoder =")
    w("    fn(&[u8]) -> Result<Box<dyn ProtobufBody>, prost::DecodeError>;")
    w("")
    w("macro_rules! decoders {")
    w("    ($($fn_name:ident => $ty:ty),* $(,)?) => {")
    w("        $(")
    w("            /// Decodes a body of the matching protobuf type.")
    w("            #[allow(non_snake_case)]")
    w("            pub fn $fn_name(bytes: &[u8]) -> Result<Box<dyn ProtobufBody>, prost::DecodeError> {")
    w("                Ok(Box::new(<$ty>::decode(bytes)?))")
    w("            }")
    w("        )*")
    w("    };")
    w("}")
    w("")

    steam_entries = []
    for pname in proto_names["steam"]:
        rname = mapping["steam"].get(pname)
        if rname is None:
            continue
        steam_entries.append((rname, f"decode_steam_{rname}"))
    w("decoders! {")
    for rname, fn in steam_entries:
        w(f"    {fn} => steamkit_protos::steam::{rname},")
    w("}")
    w("")

    w("decoders! {")
    for module in GC_MODULES:
        for _pname, rname in gc_tables[module]:
            w(f"    decode_{module}_{rname} => steamkit_protos::{module}::{rname},")
    w("}")
    w("")

    w("decoders! {")
    for module, service, method, req, resp in services:
        w(f"    decode_{module}_service_{service}_{method}_request")
        w(f"        => steamkit_protos::{module}::{req},")
        w(f"    decode_{module}_service_{service}_{method}_response")
        w(f"        => steamkit_protos::{module}::{resp},")
    w("}")
    w("")

    w("/// The protobuf body type name and decoder for a message number.")
    w("///")
    w("/// Uses an explicit override if there is one, otherwise the first Steam")
    w("/// protobuf message whose name ends with the message name.")
    w("#[allow(deprecated)]")
    w("pub fn protobuf_body_decoder(emsg: EMsg) -> Option<(&'static str, ProtobufDecoder)> {")
    w("    match emsg {")
    for name, rname in sorted(emsg_map.items(), key=lambda kv: kv[0]):
        pname = rust_to_proto[rname]
        w(f'        EMsg::{name} => Some(("{pname}", decode_steam_{rname})),')
    w("        _ => None,")
    w("    }")
    w("}")
    w("")

    w("/// The unified messaging request and response types of a service method.")
    w("pub struct ServiceMethodTypes {")
    w("    /// The name of the protobuf request type.")
    w("    pub request_type: &'static str,")
    w("    /// The decoder of the protobuf request type.")
    w("    pub request_decoder: ProtobufDecoder,")
    w("    /// The name of the protobuf response type.")
    w("    pub response_type: &'static str,")
    w("    /// The decoder of the protobuf response type.")
    w("    pub response_decoder: ProtobufDecoder,")
    w("}")
    w("")
    w("/// Looks the request and response types of a unified messaging method up.")
    w("///")
    w("/// `service_method_name` is the target job name of the message, for example")
    w("/// `Credentials.GetAccountAuthSecret#1`.")
    w("pub fn service_method_types(service_method_name: &str) -> Option<ServiceMethodTypes> {")
    w("    let service_method_name = service_method_name.trim();")
    w("    if service_method_name.is_empty() {")
    w("        return None;")
    w("    }")
    w("")
    w("    let mut parts = service_method_name.split('.');")
    w("    let service = parts.next()?;")
    w("    let method = parts.next()?;")
    w("    let method = method.split('#').next()?;")
    w("")
    w("    match (service, method) {")
    for module, svc, meth, req, resp in services:
        w(f'        ("{svc}", "{meth}") => Some(ServiceMethodTypes {{')
        w(f'            request_type: "{req}",')
        w(f"            request_decoder: decode_{module}_service_{svc}_{meth}_request,")
        w(f'            response_type: "{resp}",')
        w(f"            response_decoder: decode_{module}_service_{svc}_{meth}_response,")
        w("        }),")
    w("        _ => None,")
    w("    }")
    w("}")
    w("")

    w("/// The game coordinator message types of the shared game coordinator namespace.")
    w("///")
    w("/// Every game ships its own copy of these messages; the `tf2` one is used.")
    w("pub static BASE_GC_TYPES: &[(&str, ProtobufDecoder)] = &[")
    for pname, rname in base_entries:
        w(f'    ("{pname}", decode_tf2_{rname}),')
    w("];")
    w("")

    for module in GC_MODULES:
        w(f"/// The game coordinator message types of the `{module}` namespace.")
        w(f"pub static {module.upper()}_GC_TYPES: &[(&str, ProtobufDecoder)] = &[")
        for pname, rname in gc_tables[module]:
            w(f'    ("{pname}", decode_{module}_{rname}),')
        w("];")
        w("")

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {OUT} ({len(lines)} lines)")


if __name__ == "__main__":
    main()
