use std::collections::HashSet;
use crate::Error;
use std::path::{Path, PathBuf};
use encoding_rs_io::{DecodeReaderBytes, DecodeReaderBytesBuilder};
use elementtree::{Element, WriteOptions};

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

pub fn collect_plugin_elements(root: &Element) -> Vec<(&Element, &Element)> {
    let mut stack = root.children().into_iter().map(|child| {(root, child)}).collect::<Vec<_>>();
    let mut ret = vec![];

    while let Some((parent, element)) = stack.pop() {
        if element.tag().name() == "plugin" {
            ret.push((parent, element));
        }else{
            stack.extend(element.children().into_iter().map(|child| {(element, child)}));
        }
    }

    ret
}

fn validate_recursively(parent: &mut Element, fomod_path: &Path) -> Vec<MissingPath> {
    let mut issues = vec![];

    parent.retain_children_mut(|child| {
        if child.tag().name() != "plugin" { // recurse until a plugin is found
            issues.extend(validate_recursively(child, fomod_path));
            return true;
        }

        let Some(mod_name) = child.get_attr("name") else { return true };

        let mut is_valid_plugin = true;
        for path in collect_paths_from_plugin(child) {
            let full_path = fomod_path.join(&path);

            if !full_path.exists() {
                issues.push(MissingPath {mod_name: mod_name.to_string(), path});
                is_valid_plugin = false;
            }
        }

        return is_valid_plugin;
    });

    issues
}

/// returns (root, issues)
/// where root is a valid XML FOMOD tree, and issues is all the issues that were resolved
pub async fn validate_fomod(fomod_path: &Path) -> Result< (Element, Vec<MissingPath>), Box<dyn Error>> {
    let module_config_path = fomod_path.join("fomod").join("ModuleConfig.xml");

    let fomod_module_config = tokio::fs::read(&module_config_path).await?;
    let reader = DecodeReaderBytesBuilder::new()
        .build(fomod_module_config.as_slice());
    
    let mut root = Element::from_reader(reader)?;
    let issues = validate_recursively(&mut root, fomod_path);
    
    Ok((root, issues))
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