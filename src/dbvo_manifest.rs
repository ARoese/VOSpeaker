use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct DBVOManifest {
    pub voice_pack_name: String,
    pub voice_pack_id: String
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_serialize(){
        let manifest = DBVOManifest {
            voice_pack_name: "example pack name".to_string(),
            voice_pack_id: "example pack id".to_string(),
        };

        let as_string = serde_json::to_string(&manifest).unwrap();
        assert_eq!(as_string, "{\"voice_pack_name\":\"example pack name\",\"voice_pack_id\":\"example pack id\"}");
    }

    #[test]
    fn test_deserialize(){
        let as_string = "{\"voice_pack_name\":\"example pack name\",\"voice_pack_id\":\"example pack id\"}".to_string();
        let manifest_correct = DBVOManifest {
            voice_pack_name: "example pack name".to_string(),
            voice_pack_id: "example pack id".to_string(),
        };
        let deserialized_manifest = serde_json::from_str::<DBVOManifest>(&as_string).unwrap();

        assert_eq!(deserialized_manifest, manifest_correct);
    }
}