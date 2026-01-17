# VOSpeaker
Generate, listen to, and package voicelines using AI backends.


## VO generation directory structure

```
project_name/
├─ expansions.toml
├─ substitutions.toml
├─ chatterbox-generator-config.toml
├─ variable_mappings.txt
├─ topics/
│  ├─ topic_file1.topic.d/
│  │  ├─ topic_file1.topic
│  │  ├─ configMap.bin
│  │  ├─ 5e03377018ec6bf3.wav
│  │  └─ ae7dc5e7ebb805ff.wav
│  ├─ topic_file2.topic.d/
│  │  ├─ topic_file2.topic
│  │  ├─ configMap.bin
│  │  ├─ 5e03377018ec6bf3.wav
│  │  └─ ae7dc5e7ebb805ff.wav
```

each wav file is named by the hash of the voiceline it represents. The text hashed is the text passed to the VO generator.
Different words = different file name. This can be opaque and prevents issues with long file names.

### <generator_name>-generator-config.toml
arbitrary file for serialized VO generator settings.

### substitutions.toml
TOML file containing a serialized HashMap<String, String> which represents words (keys) that should be replaced with
another word (or nothing) when the dialogue passed to the generator to be spoken.

### expansions.toml
TOML file containing a serialized HashMap<String, Vec<String>> which represents the possible values a given global
variable/alias could represent. The elements of the Vec replace the <global=someName> fields that appear in
dialogue lines. Each raw line is "expanded" into the permutations of the possible substitutions of its contained
globals.

### configMap.bin
stores the mapping from vo hash to the hash of the config used to generate it.
This allows distinguishing between vo files that were generated with an "old" config.
see `config_map_file.rs` for details on the format.

### *.topic
newline-separated list of dialogue lines that should be spoken

## Dialog Processing
dialog has 3 forms:
1. raw, unsubstituted
    - `I have completed the quest at <alias=questLocation>. (500 gold)`
2. substituted
   - Substitutions can create several variants of the same dialog
   - `I have completed the quest at Whiterun for the Jarl. (500 gold)`
   - `I have completed the quest at Rorikstead for the Jarl. (500 gold)`
3. Phonetically substituted and trimmed
   - Some words are very difficult to pronounce, and can be replaced with phonetic synonyms when sent to the VO generator.
   - `I have completed the quest at Rorikstead for the yarl. (500 gold)` (Jarl -> yarl)
   - Some portions of text shouldn't be spoken, and will be removed when sent to the VO generator
   - `I have completed the quest at Rorikstead for the yarl.`