use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use lazy_regex::regex;
use crate::hashes::VOHash;
use crate::topic_lines::ExplodedMember::{RawText, Substitute};

#[derive(Debug, Default)]
pub struct TopicSubstituteConfig{
    pub substitutions: HashMap<String, Vec<String>>,
    pub max_expansions: usize
}

#[derive(Debug, Clone)]
enum ExplodedMember{
    RawText(String),
    Substitute(String)
}
#[derive(Debug, Clone)]
struct ExplodedRawLine(Vec<ExplodedMember>);
impl ExplodedRawLine {

    pub fn from(line: &RawTopicLine) -> ExplodedRawLine {
        let globals_regex = regex!(r"<(?:global|alias)=(.*?)>"i);

        let mut elements: Vec<ExplodedMember> = Vec::new();
        let mut source = line.0.clone();
        // capture a match
        // shift its preceding text and the match onto the elements list
        // consume the shifted portion of source
        // repeat until there are no more matches
        while let Some(capture) = globals_regex.captures(&source) {
            let name = capture[1].to_string();
            let capture_range = &capture.get_match().range();
            if capture_range.start != 0 {
                // add the preceding raw text
                elements.push(ExplodedMember::RawText(source[..capture_range.start].to_string()));
            }
            // add the matched global
            elements.push(ExplodedMember::Substitute(name));
            // consume the portion parsed
            source.replace_range(..capture_range.end, "");
        }

        if source.len() != 0 {
            elements.push(ExplodedMember::RawText(source));
        }

        ExplodedRawLine(elements)
    }

    pub fn implode(&self) -> String {
        self.0.iter().map(|e| match e {
            RawText(txt) => {txt}
            Substitute(_) => {""}
        }).collect::<Vec<&str>>().join("")
    }

    pub fn has_substitutions(&self) -> bool {
        self.0.iter().any(|e| matches!(e, Substitute(_)))
    }

    pub fn permute(&self, substitutions: &HashMap<String, Vec<String>>) -> Vec<String> {
        // permute the first global
        let mut replaced = self.replace_first(substitutions);

        // permute all globals after that until we no longer have any left
        while replaced.iter().any(|e| e.has_substitutions()) {
            replaced = replaced.iter().flat_map(|e| e.replace_first(substitutions)).collect();
        }

        // implode all permutations into strings
        replaced.iter().map(ExplodedRawLine::implode).collect()
    }

    fn replace_first(&self, substitutions: &HashMap<String, Vec<String>>) -> Vec<ExplodedRawLine> {
        if !self.has_substitutions(){
            return vec![self.clone()];
        }

        let mut res = Vec::<ExplodedRawLine>::new();
        for (i,e) in self.0.iter().enumerate() {
            if let Substitute(name) = e {
                if let Some(rep_list) = substitutions.get(name) {
                    for rep in rep_list.iter() {
                        let mut copy = self.clone();
                        copy.0[i] = RawText(rep.clone());
                        res.push(copy);
                    }
                }
            }
        }

        res
    }
}

#[derive(Debug, Clone)]
pub struct RawTopicLine(pub String);
impl RawTopicLine {
    pub fn substitute(&self, config: &TopicSubstituteConfig) -> Vec<SubstitutedTopicLine> {
        ExplodedRawLine::from(self)
            .permute(&config.substitutions)
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .take(config.max_expansions) // limit expansions
            .map(|s| SubstitutedTopicLine(s))
            .collect()
    }
}

fn without_leading_trailing_parens(line: &str) -> &str {
    let start_parens_regex = regex!(r"^(\(.*)\).*");
    let end_parens_regex = regex!(r".*(\(.*)\)$");
    let mut without_parens = line;
    if let Some(capture) = start_parens_regex.captures(line) {
        without_parens = without_parens.trim_start_matches(&capture[1]);
    }
    if let Some(capture) = end_parens_regex.captures(line) {
        without_parens = without_parens.trim_end_matches(&capture[1]);
    }
    without_parens
}

#[derive(Debug, Clone)]
pub struct SubstitutedTopicLine(pub String);
impl SubstitutedTopicLine {
    pub fn clean(&self) -> CleanTopicLine {
        let trimmed = self.0.trim()
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ");

        // TODO: fix this. It doesn't actually remove the start of line
        // TODO: and end of line parenthesized regions
        let without_parens = without_leading_trailing_parens(&trimmed).to_string();
        CleanTopicLine(
            without_parens
        )
    }
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct CleanTopicLine(pub String);
impl Display for CleanTopicLine {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl CleanTopicLine {
    pub fn vo_hash(&self) -> VOHash {
        VOHash(*md5::compute(&self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashmap;

    #[test]
    fn test_substitutions() {
        let raw_lines: Vec<_> = vec![
            "This is a dialogue line with no substitutions.",
            "This is a <alias=item> with a single substitution.",
            "This is a <global=item> with many <global=object>"
        ].into_iter().map(|s| RawTopicLine(s.to_string())).collect();

        let config = TopicSubstituteConfig {
            substitutions: hashmap!{
                "item" => vec!["line", "voiceline", "<global=invalidSub>"],
                "object" => vec!["orange", "apple"]
            }.into_iter()
                .map(
                    |(k,v)|
                        (String::from(k), v.into_iter().map(String::from).collect())
                ).collect(),
            max_expansions: 16
        };

        let subs: Vec<_> = raw_lines
            .iter().flat_map(|raw_line|{raw_line.substitute(&config)})
        .collect();

        for line in subs {
            println!("{:?}", line);
        }
    }
}