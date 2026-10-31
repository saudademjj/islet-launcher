//! Live acceptance against isolated caches; never reads launcher accounts.
use islet::core::{
    java::detect::JavaInstallation, loaders, mojang::install, paths::AppPaths,
    settings::DownloadSource,
};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).ok_or("pass isolated data root")?;
    let paths = AppPaths::from_root(root)?;
    let client = reqwest::Client::builder()
        .user_agent("Islet feature acceptance/0.4.0")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(45))
        .build()?;
    let cancel = CancellationToken::new();
    if std::env::args().nth(2).as_deref() == Some("content") {
        use islet::core::{content::ContentKind, modrinth};
        let store = islet::core::instances::InstanceStore::load(paths.clone())?;
        let instance = store.create("Datapack acceptance", "1.20.1", "release")?;
        let folder = paths
            .instance_game_dir(&instance.id)
            .join("saves/AuditSave");
        std::fs::create_dir_all(&folder)?;
        std::fs::write(folder.join("level.dat"), b"test-save-marker")?;
        let hits = modrinth::search(
            &client,
            &instance,
            "terralith",
            0,
            ContentKind::Datapack,
            &[],
            "relevance",
        )
        .await?;
        let project = hits.hits.first().ok_or("no datapack hits")?;
        let details = modrinth::details(&client, &project.project_id).await?;
        assert!(!details.body.is_empty());
        let versions = modrinth::versions(
            &client,
            &instance,
            &project.project_id,
            ContentKind::Datapack,
        )
        .await?;
        let version = versions.first().ok_or("no compatible datapack version")?;
        let _notes = modrinth::changelog(&client, &version.id).await?;
        modrinth::install_pack(
            &client,
            &paths,
            &instance,
            &version.id,
            ContentKind::Datapack,
            Some("AuditSave"),
            &cancel,
        )
        .await?;
        assert!(
            std::fs::read_dir(folder.join("datapacks"))?
                .next()
                .is_some()
        );
        println!(
            "PASS Modrinth datapack search → details → changelog → compatible version → selected save"
        );
        return Ok(());
    }
    if std::env::args().nth(2).as_deref() == Some("skin") {
        use islet::core::{auth, mojang::version};
        let plan = install::build_plan(&client, &paths, "1.12.2", DownloadSource::Official, &cancel)
            .await?;
        let tasks: Vec<_> = plan.files.iter().filter(|f| f.kind == install::FileKind::Library).map(|f| f.task.clone()).collect();
        let progress = Arc::new(islet::core::download::DownloadProgress::new(&tasks));
        islet::core::download::download_all(tasks, client.clone(), 8, cancel.clone(), progress).await?;
        let mut account = auth::offline::create_account("AuditSkin")?;
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&vec![255u8; 64 * 64 * 4])?;
        }
        auth::local_skin::save(&paths, &mut account, Some(bytes), false)?;
        let server = auth::local_skin::SkinServer::start(&paths, &account)?;
        let agent = auth::yggdrasil::ensure_agent(&client, &paths).await?;
        let java = islet::core::java::detect::find_all()
            .into_iter()
            .find(|j| j.major == 17)
            .ok_or("Java 17 required for skin acceptance")?;
        let libraries: Vec<_> = plan
            .version
            .libraries
            .iter()
            .filter_map(version::artifact_path)
            .map(|p| paths.library_path(&p))
            .filter(|p| p.is_file())
            .filter(|p| !p.to_string_lossy().contains("/authlib/"))
            .chain(std::iter::once(
                paths.library_path("com/mojang/authlib/1.5.25/authlib-1.5.25.jar"),
            ))
            .collect();
        let classpath = std::env::join_paths(libraries)?;
        let source = paths.cache.join("SkinAcceptance.java");
        std::fs::write(
            &source,
            r#"import java.net.*; import java.util.*; public class SkinAcceptance { public static void main(String[] a) throws Exception {
Class<?> auth = Class.forName("com.mojang.authlib.yggdrasil.YggdrasilAuthenticationService");
Object service = auth.getConstructor(java.net.Proxy.class, String.class).newInstance(java.net.Proxy.NO_PROXY, "0");
Object session = auth.getMethod("createMinecraftSessionService").invoke(service);
Class<?> profile = Class.forName("com.mojang.authlib.GameProfile"); Object player;
try { player = profile.getConstructor(java.util.UUID.class, String.class).newInstance(java.util.UUID.fromString(a[0]), "AuditSkin"); } catch(NoSuchMethodException e) { player = profile.getConstructor(String.class, String.class).newInstance(a[0].replace("-", ""), "AuditSkin"); }
Object filled = session.getClass().getMethod("fillProfileProperties", profile, boolean.class).invoke(session, player, false);
java.util.Map<?,?> verified = (java.util.Map<?,?>)session.getClass().getMethod("getTextures", profile, boolean.class).invoke(session, filled, true); if(verified.isEmpty()) throw new IllegalStateException("Secure texture validation failed");
Object props = profile.getMethod("getProperties").invoke(filled);
java.util.Collection<?> values = (java.util.Collection<?>) props.getClass().getMethod("get", Object.class).invoke(props, "textures");
if(values.isEmpty()) throw new IllegalStateException("No skin texture from local API");
Object prop = values.iterator().next(); String texture = (String)prop.getClass().getMethod("getValue").invoke(prop);
String json = new String(java.util.Base64.getDecoder().decode(texture), java.nio.charset.StandardCharsets.UTF_8);
if(!json.contains("/skin.png")) throw new IllegalStateException(json); System.out.println("SKIN_OK");
} }"#,
        )?;
        let output = tokio::process::Command::new(java.path)
            .arg(format!("-javaagent:{}={}", agent.display(), server.url))
            .arg("-cp")
            .arg(classpath)
            .arg(source)
            .arg(&account.uuid)
            .current_dir(&paths.cache)
            .output()
            .await?;
        if !output.status.success() {
            return Err(format!(
                "Skin protocol failed: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        assert!(String::from_utf8_lossy(&output.stdout).contains("SKIN_OK"));
        println!(
            "PASS offline skin: real authlib-injector + Mojang authlib retrieves local texture profile"
        );
        return Ok(());
    }
    if std::env::args().nth(2).as_deref() == Some("full") {
        let store = islet::core::instances::InstanceStore::load(paths.clone())?;
        for mc in ["1.7.2", "1.6.4", "1.5.2", "b1.7.3"] {
            let plan =
                install::build_plan(&client, &paths, mc, DownloadSource::Official, &cancel).await?;
            let tasks: Vec<_> = plan.files.iter().map(|f| f.task.clone()).collect();
            let progress = Arc::new(islet::core::download::DownloadProgress::new(&tasks));
            islet::core::download::download_all(
                tasks,
                client.clone(),
                16,
                cancel.clone(),
                progress,
            )
            .await?;
            let instance = store.create(&format!("Legacy {mc}"), mc, "release")?;
            let account = islet::core::auth::offline::create_account("Audit").unwrap();
            let java = JavaInstallation {
                path: "not-spawned".into(),
                major: 8,
                version: "8".into(),
                arch: plan.arch.into(),
                vendor: "acceptance".into(),
            };
            let options = islet::core::launch::LaunchOptions {
                arch: plan.arch.into(),
                ..Default::default()
            };
            let spec = islet::core::launch::build_spec(
                &paths,
                &instance,
                &plan.version,
                &java,
                &islet::core::auth::offline::session(&account),
                "",
                2048,
                &options,
            )?;
            assert!(spec.args.iter().all(|a| !a.contains("${")));
            let index_id =
                islet::core::mojang::version::version_asset_index_id(&plan.version).unwrap();
            let index: islet::core::mojang::version::AssetIndexJson =
                serde_json::from_slice(&std::fs::read(paths.asset_index(index_id))?)?;
            let root = if index.map_to_resources {
                spec.game_dir.join("resources")
            } else {
                paths.assets.join("virtual").join(index_id)
            };
            if index.virtual_ || index.map_to_resources {
                for name in index.objects.keys() {
                    assert!(root.join(name).is_file(), "Missing named asset {name}");
                }
            }
            println!(
                "PASS full vanilla {mc}: downloads, native extraction, launch args, {} named assets",
                index.objects.len()
            );
        }
        return Ok(());
    }
    if std::env::args().nth(2).as_deref() == Some("extra") {
        for loader in ["legacyfabric", "liteloader", "cleanroom"] {
            let mc = if loader == "legacyfabric" {
                "1.8.9"
            } else {
                "1.12.2"
            };
            let plan =
                install::build_plan(&client, &paths, mc, DownloadSource::Official, &cancel).await?;
            let (version, tasks, pinned) = if loader == "legacyfabric" {
                islet::core::fabric::prepare_flavor(
                    islet::core::fabric::Flavor::Legacy,
                    &client,
                    &paths,
                    &plan.version,
                    None,
                    &cancel,
                )
                .await?
            } else {
                islet::core::extra_loaders::prepare(
                    &client,
                    &paths,
                    &plan.version,
                    loader,
                    None,
                    &cancel,
                )
                .await?
            };
            if loader == "legacyfabric" {
                assert!(
                    tasks
                        .iter()
                        .any(|t| t.dest.to_string_lossy().contains("natives"))
                );
            }
            if loader == "cleanroom" {
                assert!(islet::core::mojang::version::java_major(&version) >= 21);
                assert!(
                    !version
                        .libraries
                        .iter()
                        .any(|l| l.name.starts_with("org.lwjgl.lwjgl:"))
                );
            }
            let progress = Arc::new(islet::core::download::DownloadProgress::new(&tasks));
            islet::core::download::download_all(
                tasks,
                client.clone(),
                8,
                cancel.clone(),
                progress,
            )
            .await?;
            println!(
                "PASS {loader} {pinned}: real profile and dependency downloads, Java {}",
                islet::core::mojang::version::java_major(&version)
            );
        }
        return Ok(());
    }
    if std::env::args().nth(2).as_deref() == Some("optifine") {
        let plan =
            install::build_plan(&client, &paths, "1.12.2", DownloadSource::Official, &cancel)
                .await?;
        let tasks: Vec<_> = plan
            .files
            .iter()
            .filter(|f| f.kind == install::FileKind::Client)
            .map(|f| f.task.clone())
            .collect();
        let progress = Arc::new(islet::core::download::DownloadProgress::new(&tasks));
        islet::core::download::download_all(
            tasks,
            client.clone(),
            1,
            cancel.clone(),
            progress,
        )
        .await?;
        let store = islet::core::instances::InstanceStore::load(paths.clone())?;
        let mut instance = store.create("OptiFine acceptance", "1.12.2", "release")?;
        instance.installed = true;
        store.update(&instance)?;
        let file =
            islet::core::optifine::install_vanilla(&client, &paths, &instance, &cancel)
                .await?;
        println!("PASS vanilla OptiFine: {file}, merged launch profile generated");
        return Ok(());
    }
    let java = JavaInstallation {
        path: "must-not-run-legacy-installers".into(),
        version: "8".into(),
        major: 8,
        arch: "x86_64".into(),
        vendor: "acceptance".into(),
    };
    for (mc, pinned) in [
        ("1.7.2", None),
        ("1.6.4", None),
        ("1.7.10", Some("10.13.4.1614-1.7.10")),
        ("1.8.9", Some("11.15.1.2318-1.8.9")),
        ("1.10.2", Some("12.18.3.2511")),
        ("1.12.2", Some("14.23.4.2705")),
    ] {
        let plan =
            install::build_plan(&client, &paths, mc, DownloadSource::Official, &cancel).await?;
        assert!(!plan.files.is_empty());
        println!("PASS vanilla metadata {mc}: {} downloads", plan.files.len());
        if let Some(pinned) = pinned {
            let (merged, _) = loaders::install(
                &client,
                &paths,
                &plan.version,
                "forge",
                Some(pinned),
                &java,
                &cancel,
                Arc::new(|_, _, _| {}),
            )
            .await?;
            for library in &merged.libraries {
                // The new dependencies were really fetched. Vanilla dependencies
                // are tested by their official plan above, without downloading assets.
                if !plan
                    .version
                    .libraries
                    .iter()
                    .any(|l| l.name == library.name)
                {
                    let path = islet::core::mojang::version::artifact_path(library).unwrap();
                    assert!(paths.library_path(&path).is_file(), "{path}");
                }
            }
            assert_eq!(merged.main_class, "net.minecraft.launchwrapper.Launch");
            println!(
                "PASS Forge {mc}-{pinned}: extracted profile, downloaded dependencies, no Java GUI"
            );
        }
    }
    for (loader, mc) in [
        ("legacyfabric", "1.8.9"),
        ("liteloader", "1.12.2"),
        ("cleanroom", "1.12.2"),
    ] {
        let versions = loaders::list(&client, loader, mc).await?;
        assert!(!versions.is_empty());
        println!("PASS {loader} metadata: {}", versions[0].version);
    }
    let result =
        islet::core::modrinth::search_modpacks(&client, "", 0, Some("1.20.1"), None, None, "downloads")
            .await?;
    assert!(!result.hits.is_empty());
    let versions =
        islet::core::modrinth::modpack_versions(&client, &result.hits[0].project_id).await?;
    assert!(!versions.is_empty());
    let file = islet::core::modrinth::modpack_file(&client, &versions[0].id).await?;
    println!("PASS Modrinth browse → versions → mrpack: {}", file.1);
    Ok(())
}
