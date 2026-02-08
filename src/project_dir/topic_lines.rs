use crate::project_dir::hashes::VOHash;
use crate::project_dir::topic_lines::ExplodedMember::{RawText, Substitute};
use crate::project_dir::topic_lines::SentenceFragment::{DecoratedWord, Word};
use lazy_regex::regex;
use serde::{Deserialize, Serialize};
use std::cmp::max;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TopicExpansionConfig {
    pub expansions: HashMap<String, Vec<String>>,
    pub max_expansions: usize
}

impl TopicExpansionConfig {
    pub fn merge_with(&self, other: &TopicExpansionConfig) -> Self {
        let mut new_expansions = self.expansions.clone();
        for (key, value) in other.expansions.clone() {
            new_expansions.entry(key)
                .and_modify(|existing_value| existing_value.extend(value.clone()))
                .or_insert(value);
        }

        TopicExpansionConfig {
            max_expansions: max(self.max_expansions, other.max_expansions),
            expansions: new_expansions
        }
    }
}

#[derive(Debug, Clone)]
pub enum ExplodedMember{
    RawText(String),
    Substitute(String)
}

pub fn explode_raw_line(line: &str) -> ExplodedRawLine {
    let globals_regex = regex!(r"<\s*(?:global|alias)?\s*(?:\.\s*(?<suffix>.+?))?\s*=\s*(?<basename>.+?\s*)>|<(?<basename2>\w+)>"i);

    let mut elements: Vec<ExplodedMember> = Vec::new();
    let mut source = line;
    // capture a match
    // shift its preceding text and the match onto the elements list
    // consume the shifted portion of source
    // repeat until there are no more matches
    while let Some(capture) = globals_regex.captures(&source) {
        let basename = {
            let basename = capture.name("basename");
            let basename2 = capture.name("basename2");
            if let Some(basename2) = basename2 {
                basename2.as_str()
            }else{
                basename.expect("If basename2 is None, then basename MUST be some.").as_str()
            }
        };

        let derived_name = if let Some(suffix) = capture.name("suffix") {
            format!("{}.{}", basename, suffix.as_str())
        }else{
            basename.to_string()
        };
        let capture_range = &capture.get_match().range();
        if capture_range.start != 0 {
            // add the preceding raw text
            elements.push(RawText(source[..capture_range.start].to_string()));
        }
        // add the matched global
        elements.push(Substitute(derived_name));
        // consume the portion parsed
        source = &source[capture_range.end..];
        // source.replace_range(..capture_range.end, "");
    }

    if source.len() != 0 {
        elements.push(RawText(source.to_string()));
    }

    ExplodedRawLine(elements)
}

#[derive(Debug, Clone)]
pub struct ExplodedRawLine(pub Vec<ExplodedMember>);
impl ExplodedRawLine {
    pub fn from_str(line: &str) -> ExplodedRawLine {
        explode_raw_line(line)
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

    /// returns the set of all globals used in the line
    pub fn global_names(&self) -> HashSet<String> {
        self.0.iter().filter_map(|part|
            match part {
                RawText(_) => None,
                Substitute(name) => Some(name.clone())
            }
        ).collect::<_>()
    }

    pub fn permute(&self, substitutions: &HashMap<String, Vec<String>>) -> Vec<String> {
        // permute all globals after that until we no longer have any left
        let first_name = self.0.iter().find_map(|part|
            match part {
                Substitute(name) => Some(name),
                _ => None
            }
        );

        // for any recursive tree, ALL leaves will take the same base case
        let Some(first_name) = first_name else {
            // BASE CASE: if there is nothing to substitute,
            // implode the line to a string, and return it
            return vec![self.implode()]
        };

        let Some(substitutions_vec) = substitutions.get(first_name) else {
            // BASE CASE: if we encounter a substitution with no
            // replacements, then return nothing. This will eventually make the
            // whole recursive call return empty vec because all leaves will return here
            return vec![];
        };

        // RECURSIVE CASE: The replace_all_of call replaces the first global with each of its
        // possible substitutions. This means that the recursive calls have one less global
        // to replace. This means all calls will eventually return
        let mut expanded: Vec<String> = substitutions_vec
            .iter()
            .flat_map(|substitution|
                self.replace_all_of(first_name, substitution).permute(&substitutions)
            ).into_iter().collect();
        expanded.dedup(); // remove duplicate lines in a stable way
        expanded
    }

    /// replace all Substitute(target) with RawLine(replacement)
    fn replace_all_of(&self, target: &String, replacement: &String) -> ExplodedRawLine {
        let mut clone = self.clone();
        for elem in clone.0.iter_mut().filter(|e| matches!(e, Substitute(name) if name == target)) {
            *elem = RawText(replacement.clone());
        }
        clone
    }
}

#[derive(Debug, Clone)]
pub struct RawTopicLine(pub ExplodedRawLine);
impl RawTopicLine {
    pub fn new(str: &str) -> RawTopicLine {
        RawTopicLine(ExplodedRawLine::from_str(str))
    }

    pub fn substitute(&self, config: &TopicExpansionConfig) -> Vec<SubstitutedTopicLine> {
        self.0
            .permute(&config.expansions)
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            // limit expansions, but always take at least 1
            .take(max(1, config.max_expansions))
            .map(|s| SubstitutedTopicLine(s, self.0.clone()))
            .collect()
    }
}

// TODO: This has problems if there is a period. Ex. "Some dialogue (something that should be removed)."
// TODO: This has problems with multiple sets of parens Ex. "I'll do it. (Cast turn Undead) (15 Gold)"
fn without_leading_trailing_parens(line: &str) -> &str {
    let start_parens_regex = regex!(r"^(\(.*?\)).*");
    let end_parens_regex = regex!(r".*(\(.*?\))$");
    let mut without_parens = line;
    if let Some(capture) = start_parens_regex.captures(line) {
        without_parens = without_parens.trim_start_matches(&capture[1]);
    }
    if let Some(capture) = end_parens_regex.captures(line) {
        without_parens = without_parens.trim_end_matches(&capture[1]);
    }
    without_parens
}

enum SentenceFragment {
    Word(String),
    DecoratedWord{
        prefix: String,
        word: String,
        suffix: String
    }
}

struct Sentence {
    words: Vec<SentenceFragment>,
}

impl Sentence {
    pub fn from_string(input: &str) -> Sentence {
        let decorative_punctuation = vec!['.', ',', ';', ':', '"', '\'', '`', '*', '!', '?', '/', '\\', '|', '_'];
        let words = input.split(" ")
            .map(|w| {
                if !w.chars().any(|c| decorative_punctuation.contains(&c)) {
                    Word(w.to_string())
                }else{
                    let chars = w.chars().collect::<Vec<_>>();

                    let prefix_end = chars.iter().position(|c| !decorative_punctuation.contains(&c));
                    let mut prefix = String::new();
                    let without_prefix = if let Some(prefix_end) = prefix_end {
                        prefix = chars[..prefix_end].iter().collect();
                        &chars[prefix_end..]
                    }else{
                        &chars[..]
                    };
                    let mut suffix = String::new();
                    let suffix_start = without_prefix.iter().rposition(|c| !decorative_punctuation.contains(&c));
                    let without_suffix = if let Some(suffix_start) = suffix_start && suffix_start != without_prefix.len()-1 {
                        suffix = without_prefix[suffix_start+1..].iter().collect();
                        &without_prefix[..suffix_start+1]
                    }else{
                        &without_prefix[..]
                    };


                    let word = without_suffix.iter().collect::<String>();
                    DecoratedWord{prefix, word, suffix}
                }
            }).collect();

        Sentence{words}
    }
}

#[derive(Debug, Clone)]
pub struct SubstitutedTopicLine(pub String, ExplodedRawLine);
impl SubstitutedTopicLine {

    fn perform_substitutions(original: &String, substitutions: HashMap<String, String>) -> String {
        let lower_substitutions = substitutions.iter().map(|(k,v)| (k.to_lowercase(), v)).collect::<HashMap<_,_>>();
        let sentence = Sentence::from_string(original);

        sentence.words
            .into_iter()
            .map(|frag| {
                match frag {
                    Word(w) => {
                        lower_substitutions.get(&w.to_lowercase()).cloned().cloned().unwrap_or(w)
                    }
                    DecoratedWord { prefix, word, suffix } => {
                        let new_word = lower_substitutions.get(&word.to_lowercase()).cloned().cloned().unwrap_or(word);
                        format!("{}{}{}", prefix, new_word, suffix)
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn remove_bracketed(original: &str) -> String {
        let bracket_regex = regex!(r"\[.*?\]");
        bracket_regex.replace_all(original, "").to_string()
    }

    pub fn spoken(&self, substitutions: &HashMap<String, String>) -> SpokenTopicLine {
        let trimmed = self.0.trim()
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ");

        let without_brackets = Self::remove_bracketed(&trimmed);

        // continuously trim parenthesized portions out of the dialogue until doing so results in no change.
        // TODO: This is very slow. Prefer a parsing approach.
        let mut without_parens = without_leading_trailing_parens(&without_brackets).to_string().trim().to_string();
        let mut without_parens_next = without_leading_trailing_parens(&without_parens).to_string().trim().to_string();
        while without_parens_next != without_parens {
            without_parens = without_parens_next.to_string();
            without_parens_next = without_leading_trailing_parens(&without_parens).to_string();
        }
        
        let substituted = Self::perform_substitutions(&without_parens, substitutions.clone());
        SpokenTopicLine(
            substituted
        )
    }
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SpokenTopicLine(pub String);
impl Display for SpokenTopicLine {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl SpokenTopicLine {
    pub fn vo_hash(&self) -> VOHash {
        VOHash(*md5::compute(&self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashmap;

    #[test]
    fn test_expansions() {
        let raw_lines: Vec<_> = vec![
            "This is a dialogue line with no substitutions.",
            "This is a <alias=item> with a single substitution.",
            "This is a <global=item> with many <global=object>"
        ].into_iter().map(|s| RawTopicLine::new(s)).collect();

        let config = TopicExpansionConfig {
            expansions: hashmap!{
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

    #[test]
    fn test_substitutions() {
        let raw_lines: Vec<_> = vec![
            "(pre-text parens) This is a dialogue line with no substitutions. (post-text parens)",
        ].into_iter().map(|s| RawTopicLine::new(s)).collect();

        let config = TopicExpansionConfig {
            expansions: hashmap!{
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

        let substitutions = hashmap!{
            "dialogue".to_string() => "chat".to_string()
        };

        for line in subs.iter() {
            let spoken = line.spoken(&substitutions);
            println!("{:?}", spoken);
        }
    }

    #[test]
    fn test_substitutions2() {
        let substitutions = HashMap::<String, String>::new();

        let sentence1 = Sentence::from_string("not sure that \"pleased\" is the right word, friend");
        let sentence2 = Sentence::from_string("do you know \"The age of Aggression\", perhaps?");
        let mark = 1+1;
    }
}