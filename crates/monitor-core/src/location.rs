//! `wrld_xxx:12345~hidden(usr_xxx)~region(jp)` 形式のインスタンス情報を読み取る。

use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub world_id: String,
    pub instance_name: String,
    pub access: String,
    pub region: String,
}

impl Location {
    pub fn parse(raw: &str) -> Self {
        let (world_id, rest) = raw.split_once(':').unwrap_or((raw, ""));
        let mut parts = rest.split('~');
        let instance_name = parts.next().unwrap_or_default();
        let tags: HashMap<&str, &str> = parts.map(parse_tag).collect();
        Self {
            world_id: world_id.to_string(),
            instance_name: instance_name.to_string(),
            access: access_label(&tags).to_string(),
            // リージョンの指定が無いインスタンスは、VRChat では US として扱われる
            region: tags.get("region").unwrap_or(&"us").to_uppercase(),
        }
    }
}

/// `name(value)` または `name` 形式のタグを分解する。
fn parse_tag(tag: &str) -> (&str, &str) {
    match tag.split_once('(') {
        Some((name, value)) => (name, value.strip_suffix(')').unwrap_or(value)),
        None => (tag, ""),
    }
}

fn access_label(tags: &HashMap<&str, &str>) -> &'static str {
    if tags.contains_key("group") {
        return match tags.get("groupAccessType").copied() {
            Some("public") => "Group Public",
            Some("plus") => "Group+",
            _ => "Group",
        };
    }
    if tags.contains_key("hidden") {
        "Friends+"
    } else if tags.contains_key("friends") {
        "Friends"
    } else if tags.contains_key("private") {
        if tags.contains_key("canRequestInvite") {
            "Invite+"
        } else {
            "Invite"
        }
    } else {
        "Public"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORLD: &str = "wrld_00000000-0000-0000-0000-000000000001:10129";
    const USER: &str = "usr_33333333-3333-3333-3333-333333333333";

    fn check(tags: &str, access: &str, region: &str) {
        let location = Location::parse(&format!("{WORLD}{tags}"));
        assert_eq!(location.access, access, "{tags}");
        assert_eq!(location.region, region, "{tags}");
        assert_eq!(location.instance_name, "10129", "{tags}");
    }

    #[test]
    fn reads_every_access_type() {
        check("~region(jp)", "Public", "JP");
        check(&format!("~hidden({USER})~region(jp)"), "Friends+", "JP");
        check(&format!("~friends({USER})~region(jp)"), "Friends", "JP");
        check(
            &format!("~private({USER})~canRequestInvite~region(jp)"),
            "Invite+",
            "JP",
        );
        check(&format!("~private({USER})~region(eu)"), "Invite", "EU");
        check(
            "~group(grp_1)~groupAccessType(public)~region(jp)",
            "Group Public",
            "JP",
        );
        check(
            "~group(grp_1)~groupAccessType(plus)~region(jp)",
            "Group+",
            "JP",
        );
        check(
            "~group(grp_1)~groupAccessType(members)~region(jp)",
            "Group",
            "JP",
        );
    }

    #[test]
    fn region_defaults_to_us() {
        check(&format!("~private({USER})~nonce(abc)"), "Invite", "US");
    }

    #[test]
    fn keeps_world_id() {
        let location = Location::parse(WORLD);
        assert_eq!(
            location.world_id,
            "wrld_00000000-0000-0000-0000-000000000001"
        );
    }
}
