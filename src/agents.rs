//! The agents file, which says who plays: each `[[agent]]` names a player, its skin, and the script compiled to WASM that
//! drives it.

use std::path::Path;

use sim::avatar::Skin;
use toml::{Table, Value};

/// What the agents file calls each skin.
const SKINS: [(&str, Skin); 4] = [
    ("player", Skin::Player),
    ("autumn", Skin::Autumn),
    ("cyber-a", Skin::CyberA),
    ("cyber-b", Skin::CyberB),
];

#[derive(PartialEq, Debug)]
pub struct Spec {
    pub name: String,
    pub skin: Skin,
    /// Relative to the current folder, wherever the file was.
    pub script: std::path::PathBuf,
}

/// The agents in the file at `path`. Its script paths are relative to the file.
pub fn read(path: &Path) -> Result<Vec<Spec>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    parse(&text, path.parent().unwrap_or(Path::new("")))
}

fn parse(text: &str, folder: &Path) -> Result<Vec<Spec>, String> {
    let file: Table = text.parse().map_err(|error: toml::de::Error| error.to_string())?;
    only_keys(&file, &["agent"], "the file")?;
    let Some(agents) = file.get("agent") else {
        return Ok(Vec::new());
    };
    let agents = agents.as_array().ok_or("agents are written as [[agent]] tables")?;
    let mut specs: Vec<Spec> = Vec::new();
    for (number, agent) in (1..).zip(agents) {
        let agent = agent.as_table().ok_or("agents are written as [[agent]] tables")?;
        let place = format!("agent {number}");
        only_keys(agent, &["name", "skin", "script"], &place)?;
        let text = |key| {
            agent
                .get(key)
                .and_then(Value::as_str)
                .ok_or(format!("{place} needs a {key} in quotes"))
        };
        let name = text("name")?;
        if specs.iter().any(|spec| spec.name == name) {
            return Err(format!("{place} has the same name as another, {name:?}"));
        }
        let skin = text("skin")?;
        let skin = SKINS
            .iter()
            .find(|(known, _)| *known == skin)
            .map(|&(_, skin)| skin)
            .ok_or_else(|| {
                let known: Vec<&str> = SKINS.iter().map(|(known, _)| *known).collect();
                format!("{place}'s skin {skin:?} isn't one of {}", known.join(", "))
            })?;
        specs.push(Spec {
            name: name.to_owned(),
            skin,
            script: folder.join(text("script")?),
        });
    }
    Ok(specs)
}

/// Catches misspelt keys, which would otherwise be ignored without a word.
fn only_keys(table: &Table, known: &[&str], place: &str) -> Result<(), String> {
    match table.keys().find(|key| !known.contains(&key.as_str())) {
        Some(key) => Err(format!("{place} has an unknown key {key:?}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_agent_has_a_name_skin_and_script_beside_the_file() {
        let text = r#"
            [[agent]]
            name = "Wanda"
            skin = "autumn"
            script = "scripts/wander.wasm"

            [[agent]]
            name = "Pip"
            skin = "cyber-b"
            script = "pillar.wasm"
        "#;
        assert_eq!(
            parse(text, Path::new("game")),
            Ok(vec![
                Spec {
                    name: "Wanda".to_owned(),
                    skin: Skin::Autumn,
                    script: Path::new("game").join("scripts/wander.wasm"),
                },
                Spec {
                    name: "Pip".to_owned(),
                    skin: Skin::CyberB,
                    script: Path::new("game").join("pillar.wasm"),
                },
            ])
        );
    }

    #[test]
    fn an_empty_file_has_no_agents() {
        assert_eq!(parse("", Path::new("")), Ok(Vec::new()));
    }

    #[test]
    fn mistakes_are_errors() {
        for text in [
            "agent = 3",
            "agents = []",
            "[[agent]]\nname = \"A\"\nskin = \"player\"",
            "[[agent]]\nname = \"A\"\nskin = \"steve\"\nscript = \"a.wasm\"",
            "[[agent]]\nname = \"A\"\nskin = \"player\"\nscript = \"a.wasm\"\ncolour = \"red\"",
            "[[agent]]\nname = 1\nskin = \"player\"\nscript = \"a.wasm\"",
            "[[agent]]\nname = \"A\"\nskin = \"player\"\nscript = \"a.wasm\"\n[[agent]]\nname = \"A\"\nskin = \"autumn\"\nscript = \"b.wasm\"",
            "[[agent]\n",
        ] {
            assert!(parse(text, Path::new("")).is_err(), "{text}");
        }
    }
}
