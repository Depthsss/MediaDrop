use std::env;

const MSI_ENV: &[&str] = &[
    "MEDIADROP_MSI_SIZE",
    "MEDIADROP_MSI_SHA256",
    "MEDIADROP_MSI_PRODUCT_NAME",
    "MEDIADROP_MSI_MANUFACTURER",
    "MEDIADROP_MSI_PRODUCT_VERSION",
    "MEDIADROP_MSI_UPGRADE_CODE",
    "MEDIADROP_MSI_TEMPLATE",
];

fn main() {
    for variable in MSI_ENV {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    println!("cargo:rerun-if-changed=worker.manifest");
    println!("cargo:rerun-if-changed=worker.rc");
    println!("cargo:rerun-if-changed=component-worker.rc");
    println!("cargo:rerun-if-changed=../../src-tauri/icons/icon.ico");

    let production_installer = env::var("PROFILE").as_deref() == Ok("release")
        && env::var_os("CARGO_FEATURE_INSTALLER_MODE").is_some()
        && env::var_os("CARGO_FEATURE_TEST_ENGINE").is_none();
    if production_installer {
        for variable in MSI_ENV {
            let value = env::var(variable)
                .unwrap_or_else(|_| panic!("production worker requires {variable}"));
            assert!(
                !value.trim().is_empty(),
                "production worker requires non-empty {variable}"
            );
        }
    }

    let resource = if env::var_os("CARGO_FEATURE_COMPONENT_MODE").is_some()
        && env::var_os("CARGO_FEATURE_INSTALLER_MODE").is_none()
    {
        "component-worker.rc"
    } else {
        "worker.rc"
    };
    embed_resource::compile(resource, embed_resource::NONE)
        .manifest_required()
        .expect("compile worker Windows resources");
}
