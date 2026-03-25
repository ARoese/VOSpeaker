use std::collections::HashSet;
use crate::Error;
use std::path::{Path, PathBuf};
use encoding_rs_io::{DecodeReaderBytes, DecodeReaderBytesBuilder};
use elementtree::Element;

#[derive(Debug)]
pub struct MissingPath {
    pub mod_name: String,
    pub path: PathBuf
}

pub fn collect_paths_from_plugin(plugin: &Element) -> HashSet<PathBuf> {
    let mut element_stack: Vec<&Element>= vec![plugin];
    let mut paths: HashSet<PathBuf> = HashSet::new();

    while let Some(element) = element_stack.pop() {
        let new_paths = element.attrs().filter_map(|attr| {
            match (attr.0.name(), attr.1) {
                ("path" | "source", value) => Some(PathBuf::from(value.replace("\\", "/"))),
                _ => None
            }
        });
        paths.extend(new_paths);
        element_stack.extend(element.children().into_iter());
    }

    return paths
}

pub fn collect_plugin_elements(root: &Element) -> Vec<&Element> {
    let mut stack = vec![root];
    let mut ret = vec![];

    while let Some(element) = stack.pop() {
        if element.tag().name() == "plugin" {
            ret.push(element);
        }else{
            stack.extend(element.children().into_iter());
        }
    }

    ret
}

pub async fn validate_fomod(fomod_path: &Path) -> Result<Vec<MissingPath>, Box<dyn Error>> {
    let module_config_path = fomod_path.join("fomod").join("ModuleConfig.xml");

    let fomod_module_config = tokio::fs::read(&module_config_path).await?;
    let reader = DecodeReaderBytesBuilder::new()
        .build(fomod_module_config.as_slice());
    
    let root = Element::from_reader(reader)?;
    let mut issues = vec![];
    'plugin: for plugin in collect_plugin_elements(&root) {
        let Some(mod_name) = plugin.get_attr("name") else { continue };
        let paths = collect_paths_from_plugin(plugin);

        for path in paths {
            let full_path = fomod_path.join(&path);
            if !full_path.exists() {
                issues.push(MissingPath {mod_name: mod_name.to_string(), path});
                continue 'plugin;
            }
        }
    }

    Ok(issues)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use crate::validate_fomod::validate_fomod;

    #[tokio::test]
    async fn test_collect_paths_from_plugin() {
        let fomod_path = PathBuf::from("/home/atomr/Downloads/skyrim-voicelines/packs/female-khajiit/new/FOMOD/Female Khajiit FOMOD");
        let res = validate_fomod(fomod_path.as_path()).await.unwrap();

        println!("{res:?}")
    }
}