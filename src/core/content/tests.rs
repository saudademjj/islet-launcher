use super::jar_meta::toml_logo;
use super::packs::{enabled_packs, unescape_property};
use super::*;

#[test]
fn resource_pack_order_is_written_like_the_game_does() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(dir.path()).unwrap();
    let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
    let instance = store.create("包", "1.21", "release").unwrap();
    let game = paths.instance_game_dir(&instance.id);
    fs::create_dir_all(game.join("resourcepacks/Faithful")).unwrap();
    fs::write(game.join("resourcepacks/a.zip"), b"zip").unwrap();
    assert!(set_resource_packs(&paths, &instance.id, &["a.zip".into()]).is_err(), "needs options.txt");
    fs::write(game.join("options.txt"), "fov:0.0\nresourcePacks:[\"vanilla\",\"fabric\",\"file/old.zip\"]\nlang:zh_cn\n").unwrap();
    set_resource_packs(&paths, &instance.id, &["Faithful".into(), "a.zip".into()]).unwrap();
    let text = fs::read_to_string(game.join("options.txt")).unwrap();
    assert!(text.contains(r#"resourcePacks:["vanilla","fabric","file/a.zip","file/Faithful"]"#), "{text}");
    assert!(text.contains("lang:zh_cn"));
    assert_eq!(enabled_packs(&game, ContentKind::ResourcePack), vec!["Faithful", "a.zip"]);
    assert!(set_resource_packs(&paths, &instance.id, &["../x".into()]).is_err());
}

#[test]
fn mod_hashes_are_cached_and_follow_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(dir.path()).unwrap();
    let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
    let instance = store.create("模组", "1.21", "release").unwrap();
    let mods_dir = paths.instance_game_dir(&instance.id).join("mods");
    fs::create_dir_all(&mods_dir).unwrap();
    fs::write(mods_dir.join("a.jar"), b"abc").unwrap();
    fs::write(mods_dir.join("b.jar.disabled"), b"").unwrap();
    fs::write(mods_dir.join("notes.txt"), b"x").unwrap();

    let first = mod_hashes(&paths, &instance.id).unwrap();
    assert_eq!(
        first,
        vec![
            ("a.jar".to_string(), "a9993e364706816aba3e25717850c26c9cd0d89d".to_string()),
            ("b.jar.disabled".to_string(), "da39a3ee5e6b4b0d3255bfef95601890afd80709".to_string()),
        ]
    );
    assert!(paths.instance_dir(&instance.id).join("mod-hashes.json").is_file());

    // A changed file is hashed again even though the cache has an entry.
    fs::write(mods_dir.join("a.jar"), b"abcd").unwrap();
    let file = fs::File::options().write(true).open(mods_dir.join("a.jar")).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(5)).unwrap();
    let second = mod_hashes(&paths, &instance.id).unwrap();
    assert_eq!(second[0].1, "81fe8bfe87576c3ecb22426f8e57847382917acf");
}

#[test]
fn packs_list_zips_and_folders_only() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(dir.path()).unwrap();
    let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
    let instance = store.create("资源", "1.21", "release").unwrap();
    assert!(packs(&paths, &instance.id, ContentKind::Shader).unwrap().is_empty());
    let shaders = paths.instance_game_dir(&instance.id).join("shaderpacks");
    fs::create_dir_all(shaders.join("Unpacked/shaders")).unwrap();
    fs::write(shaders.join("Unpacked/shaders/a.fsh"), b"12345").unwrap();
    fs::write(shaders.join("BSL.zip"), b"zip").unwrap();
    fs::write(shaders.join("BSL.zip.txt"), b"settings").unwrap();
    let found = packs(&paths, &instance.id, ContentKind::Shader).unwrap();
    assert_eq!(found.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["BSL.zip", "Unpacked"]);
    assert!(found[1].folder);
    assert_eq!(found[1].bytes, 5);
}

#[test]
fn packs_carry_their_icon_and_the_games_order() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(dir.path()).unwrap();
    let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
    let instance = store.create("资源", "1.21", "release").unwrap();
    let game = paths.instance_game_dir(&instance.id);
    let packs_dir = game.join("resourcepacks");
    fs::create_dir_all(packs_dir.join("Folder")).unwrap();
    fs::write(packs_dir.join("Folder/pack.png"), b"png").unwrap();
    {
        let mut zip = zip::ZipWriter::new(fs::File::create(packs_dir.join("Zipped.zip")).unwrap());
        zip.start_file("pack.png", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"zpng").unwrap();
        zip.finish().unwrap();
    }
    fs::write(packs_dir.join("Off.zip"), b"not a zip").unwrap();
    // Stored lowest priority first: Zipped wins over Folder.
    fs::write(game.join("options.txt"), "lang:zh_cn\nresourcePacks:[\"vanilla\",\"file/Folder\",\"file/Zipped.zip\"]\n").unwrap();

    let found = packs(&paths, &instance.id, ContentKind::ResourcePack).unwrap();
    let summary: Vec<_> = found.iter().map(|p| (p.name.as_str(), p.active)).collect();
    assert_eq!(summary, [("Zipped.zip", Some(1)), ("Folder", Some(2)), ("Off.zip", None)]);
    assert_eq!(found[0].icon.as_deref(), Some("data:image/png;base64,enBuZw=="));
    assert_eq!(found[1].icon.as_deref(), Some("data:image/png;base64,cG5n"));
    assert_eq!(found[2].icon, None);

    let shaders = game.join("shaderpacks");
    fs::create_dir_all(&shaders).unwrap();
    fs::write(shaders.join("BSL.zip"), b"zip").unwrap();
    fs::write(shaders.join("Other.zip"), b"zip").unwrap();
    fs::create_dir_all(game.join("config")).unwrap();
    fs::write(game.join("config/iris.properties"), "enableShaders=true\nshaderPack=BSL.zip\n").unwrap();
    let found = packs(&paths, &instance.id, ContentKind::Shader).unwrap();
    assert_eq!(found[0].name, "BSL.zip");
    assert_eq!(found[0].active, Some(1));
    assert_eq!(found[1].active, None);

    // Iris writes a Chinese name escaped.
    fs::write(shaders.join("光影 v2.zip"), b"zip").unwrap();
    fs::write(game.join("config/iris.properties"), "#Iris\nshaderPack=\\u5149\\u5F71 v2.zip\n").unwrap();
    let found = packs(&paths, &instance.id, ContentKind::Shader).unwrap();
    assert_eq!((found[0].name.as_str(), found[0].active), ("光影 v2.zip", Some(1)));
}

fn jar(path: &Path, files: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn mods_show_the_icon_from_their_jar() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(dir.path()).unwrap();
    let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
    let instance = store.create("图标", "1.21", "release").unwrap();
    let mods_dir = paths.instance_game_dir(&instance.id).join("mods");
    fs::create_dir_all(&mods_dir).unwrap();
    // Fabric by size: the one nearest 64 wins.
    jar(
        &mods_dir.join("fabric.jar"),
        &[
            ("fabric.mod.json", br#"{"id":"a","icon":{"16":"assets/a/16.png","128":"assets/a/128.png","64":"assets/a/64.png"}}"#),
            ("assets/a/64.png", b"p64"),
            ("assets/a/16.png", b"p16"),
        ],
    );
    jar(
        &mods_dir.join("forge.jar.disabled"),
        &[("META-INF/mods.toml", b"modLoader=\"javafml\"\n[[mods]]\nlogoFile = \"/logo.png\" # banner\n"), ("logo.png", b"logo")],
    );
    jar(&mods_dir.join("plain.jar"), &[("pack.png", b"pk")]);
    jar(&mods_dir.join("none.jar"), &[("fabric.mod.json", br#"{"icon":"missing.png"}"#)]);
    fs::write(mods_dir.join("broken.jar"), b"not a zip").unwrap();

    let icons = mod_icons(&paths, &instance.id).unwrap();
    let summary: Vec<_> = icons.iter().map(|(name, url)| (name.as_str(), url.as_str())).collect();
    assert_eq!(
        summary,
        [
            ("fabric.jar", "data:image/png;base64,cDY0"),
            ("forge.jar.disabled", "data:image/png;base64,bG9nbw=="),
            ("plain.jar", "data:image/png;base64,cGs="),
        ]
    );
    assert_eq!(toml_logo("logoFile='a.png'"), Some("a.png".into()));
    assert_eq!(toml_logo("logoFile=\"\""), None);
}

#[test]
fn mods_say_what_they_need() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(dir.path()).unwrap();
    let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
    let instance = store.create("依赖", "1.21", "release").unwrap();
    let mods_dir = paths.instance_game_dir(&instance.id).join("mods");
    fs::create_dir_all(&mods_dir).unwrap();
    // Fabric API answers for the modules bundled inside it.
    let module = dir.path().join("module.jar");
    jar(&module, &[("fabric.mod.json", br#"{"id":"fabric-resource-loader-v0"}"#)]);
    jar(
        &mods_dir.join("fabric-api.jar"),
        &[
            ("fabric.mod.json", br#"{"id":"fabric-api","name":"Fabric API","provides":["fabric"],"jars":[{"file":"META-INF/jars/module.jar"}]}"#),
            ("META-INF/jars/module.jar", &fs::read(&module).unwrap()),
        ],
    );
    jar(
        &mods_dir.join("sodium.jar"),
        &[("fabric.mod.json", br#"{"id":"sodium","name":"Sodium","version":"0.6","description":"Fast","depends":{"minecraft":"1.21","fabricloader":"*","fabric-resource-loader-v0":"*"}}"#)],
    );
    jar(
        &mods_dir.join("create.jar.disabled"),
        &[(
            "META-INF/neoforge.mods.toml",
            b"modLoader=\"javafml\"\n[[mods]]\nmodId=\"create\"\nversion=\"${file.jarVersion}\"\ndisplayName='Create' # name\ndescription='''\nmodId=\"not-this\"\n'''\n[[mods]]\nmodId=\"flywheel\"\n[[dependencies.create]]\nmodId=\"neoforge\"\ntype=\"required\"\n[[dependencies.create]]\nmodId=\"ponder\"\n[[dependencies.create]]\nmodId=\"jei\"\ntype=\"optional\"\n[[dependencies.create]]\nmodId=\"registrate\"\nmandatory=true\n",
        )],
    );
    // A NeoForge mod answers for the mods it bundles with jarjar.
    let ponder = dir.path().join("ponder.jar");
    jar(&ponder, &[("META-INF/neoforge.mods.toml", b"[[mods]]\nmodId=\"ponder\"\n")]);
    let create_jar = mods_dir.join("create.jar.disabled");
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    {
        let mut archive = zip::ZipArchive::new(fs::File::open(&create_jar).unwrap()).unwrap();
        let mut toml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("META-INF/neoforge.mods.toml").unwrap(), &mut toml).unwrap();
        files.push(("META-INF/neoforge.mods.toml".into(), toml.into_bytes()));
    }
    files.push(("META-INF/jarjar/metadata.json".into(), br#"{"jars":[{"path":"META-INF/jarjar/ponder.jar"}]}"#.to_vec()));
    files.push(("META-INF/jarjar/ponder.jar".into(), fs::read(&ponder).unwrap()));
    jar(&create_jar, &files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>());
    jar(&mods_dir.join("plain.jar"), &[("pack.png", b"pk")]);
    jar(
        &mods_dir.join("legacy.jar"),
        &[("mcmod.info", br#"[{"modid":"jei","name":"Just Enough Items","version":"4.16.1"}]"#)],
    );
    jar(
        &mods_dir.join("neo1204.jar"),
        &[(
            "META-INF/mods.toml",
            b"[[mods]]\nmodId=\"neo_example\"\ndisplayName=\"Neo Example\"\n[[dependencies.neo_example]]\nmodId=\"neoforge\"\nmandatory=true\n",
        )],
    );
    jar(
        &mods_dir.join("forge1201.jar"),
        &[(
            "META-INF/mods.toml",
            b"[[mods]]\nmodId=\"forge_example\"\ndisplayName=\"Forge Example\"\n[[dependencies.forge_example]]\nmodId=\"forge\"\nmandatory=true\n",
        )],
    );

    let metas = mod_meta(&paths, &instance.id).unwrap();
    assert_eq!(metas.keys().collect::<Vec<_>>(), ["create.jar.disabled", "fabric-api.jar", "forge1201.jar", "legacy.jar", "neo1204.jar", "sodium.jar"]);
    let api = &metas["fabric-api.jar"];
    assert_eq!(api.provides, ["fabric", "fabric-resource-loader-v0"]);
    assert_eq!(api.loaders, ["fabric"]);
    let sodium = &metas["sodium.jar"];
    assert_eq!((sodium.name.as_deref(), sodium.version.as_deref(), sodium.description.as_deref()), (Some("Sodium"), Some("0.6"), Some("Fast")));
    assert_eq!(sodium.depends, ["fabric-resource-loader-v0"]);
    assert_eq!(sodium.loaders, ["fabric"]);
    let create = &metas["create.jar.disabled"];
    assert_eq!((create.id.as_deref(), create.name.as_deref(), create.version.as_deref()), (Some("create"), Some("Create"), None));
    assert_eq!(create.provides, ["flywheel", "ponder"]);
    assert_eq!(create.depends, ["registrate"]);
    assert_eq!(create.loaders, ["neoforge"]);
    let legacy = &metas["legacy.jar"];
    assert_eq!((legacy.id.as_deref(), legacy.name.as_deref(), legacy.version.as_deref()), (Some("jei"), Some("Just Enough Items"), Some("4.16.1")));
    assert_eq!(legacy.loaders, ["forge"]);
    assert_eq!(metas["neo1204.jar"].loaders, ["neoforge"]);
    assert_eq!(metas["forge1201.jar"].loaders, ["forge"]);
}

#[test]
fn zipped_packs_are_hashed_for_identification() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(dir.path()).unwrap();
    let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
    let instance = store.create("哈希", "1.21", "release").unwrap();
    let game = paths.instance_game_dir(&instance.id);
    fs::create_dir_all(game.join("resourcepacks/Folder")).unwrap();
    fs::create_dir_all(game.join("shaderpacks")).unwrap();
    fs::write(game.join("resourcepacks/A.zip"), b"abc").unwrap();
    fs::write(game.join("shaderpacks/S.ZIP"), b"abc").unwrap();
    let mut found = pack_hashes(&paths, &instance.id).unwrap();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    let sha = "a9993e364706816aba3e25717850c26c9cd0d89d";
    assert_eq!(found, vec![(ContentKind::ResourcePack, "A.zip".into(), sha.into()), (ContentKind::Shader, "S.ZIP".into(), sha.into())]);
}

#[test]
fn properties_values_are_unescaped() {
    assert_eq!(unescape_property(r"\u5149\u5F71\:x\\y"), "光影:x\\y");
    assert_eq!(unescape_property(r"\uD83D\uDE00"), "😀");
    assert_eq!(unescape_property(r"bad\u12"), r"bad\u12");
}

#[test]
fn worlds_lists_real_saves_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(dir.path()).unwrap();
    let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
    let instance = store.create("世界", "1.21", "release").unwrap();
    assert!(worlds(&paths, &instance.id).unwrap().is_empty());

    let saves = paths.instance_game_dir(&instance.id).join("saves");
    for name in ["旧世界", "新世界"] {
        fs::create_dir_all(saves.join(name).join("region")).unwrap();
        fs::write(saves.join(name).join("level.dat"), b"0123456789").unwrap();
        fs::write(saves.join(name).join("region/r.0.0.mca"), vec![0u8; 100]).unwrap();
    }
    fs::write(saves.join("新世界/icon.png"), b"png").unwrap();
    // A folder without level.dat is not a world.
    fs::create_dir_all(saves.join("杂物")).unwrap();
    let old = fs::File::options().write(true).open(saves.join("旧世界/level.dat")).unwrap();
    old.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000)).unwrap();

    let found = worlds(&paths, &instance.id).unwrap();
    assert_eq!(found.iter().map(|w| w.folder.as_str()).collect::<Vec<_>>(), ["新世界", "旧世界"]);
    assert_eq!(found[0].bytes, 113);
    assert_eq!(found[0].icon.as_deref(), Some("data:image/png;base64,cG5n"));
    assert!(found[1].icon.is_none());
    assert_eq!(found[1].modified_ms, Some(1_000_000));

    assert!(world_dir(&paths, &instance.id, "新世界").is_ok());
    assert!(world_dir(&paths, &instance.id, "杂物").is_err());
    assert!(world_dir(&paths, &instance.id, "../新世界").is_err());
    assert!(world_dir(&paths, &instance.id, "新世界/region").is_err());
}

#[test]
fn restore_keeps_pre_restore_data_as_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(dir.path()).unwrap();
    let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
    let instance = store.create("世界", "1.21", "release").unwrap();
    let world = paths.instance_game_dir(&instance.id).join("saves/世界");
    fs::create_dir_all(&world).unwrap();
    fs::write(world.join("level.dat"), b"old world").unwrap();
    let saved = backup(&paths, &instance).unwrap();
    fs::write(world.join("level.dat"), b"new world").unwrap();
    let preserved = restore_backup(&paths, &instance, &saved).unwrap();
    assert_eq!(fs::read(world.join("level.dat")).unwrap(), b"old world");
    let mut zip = zip::ZipArchive::new(fs::File::open(preserved).unwrap()).unwrap();
    let mut data = Vec::new();
    std::io::Read::read_to_end(
        &mut zip.by_name("minecraft/saves/世界/level.dat").unwrap(),
        &mut data,
    )
    .unwrap();
    assert_eq!(data, b"new world");
}
