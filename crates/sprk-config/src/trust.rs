//! Whether `%ProgramData%\SprkNotes\policy.json` really comes from an administrator.
//!
//! Ordinary users may create folders in `%ProgramData%`, so a user could make the `SprkNotes`
//! folder first and drop their own `policy.json` in it (pointing other people's transcripts at a
//! folder they can read, say). The file is used only when it and its folder are owned by SYSTEM,
//! Administrators or TrustedInstaller and nobody else may write, replace or re-permission them.
//! The deployment script sets exactly that. The Windows reading lives behind [`SecurityReader`]
//! so the rules here are tested anywhere.
use std::path::Path;

/// One allow entry of an access list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ace {
    /// String form, `S-1-5-32-545`.
    pub sid: String,
    pub mask: u32,
    /// Only passed on to children; grants nothing on this object.
    pub inherit_only: bool,
}

/// Owner and allow entries of a file or folder. `dacl: None` is a null access list (everyone may
/// do anything).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityInfo {
    pub owner: String,
    pub dacl: Option<Vec<Ace>>,
}

pub trait SecurityReader {
    fn read(&self, path: &Path) -> Result<SecurityInfo, String>;
}

const SYSTEM: &str = "S-1-5-18";
const ADMINISTRATORS: &str = "S-1-5-32-544";
const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
/// Placeholder that only matters when inherited; it grants nothing on the object itself.
const CREATOR_OWNER: &str = "S-1-3-0";

/// Rights that let someone change the file, put another one in its place, or change who may.
/// For a folder 0x2 and 0x4 are "add file" and "add folder", 0x40 "delete child".
const WRITE_RIGHTS: u32 = 0x0000_0002 // write data / add file
    | 0x0000_0004 // append data / add subfolder
    | 0x0000_0040 // delete child
    | 0x0001_0000 // DELETE
    | 0x0004_0000 // WRITE_DAC
    | 0x0008_0000 // WRITE_OWNER
    | 0x1000_0000 // GENERIC_ALL
    | 0x4000_0000; // GENERIC_WRITE

fn admin(sid: &str) -> bool {
    sid == SYSTEM || sid == ADMINISTRATORS || sid == TRUSTED_INSTALLER
}

/// Ok when only administrators can change this object; otherwise why not, in plain words.
pub fn judge(what: &str, info: &SecurityInfo) -> Result<(), String> {
    if !admin(&info.owner) {
        return Err(format!(
            "{what} is owned by {} rather than SYSTEM or Administrators",
            info.owner
        ));
    }
    let Some(aces) = &info.dacl else {
        return Err(format!(
            "{what} has no access list, so anyone can change it"
        ));
    };
    for a in aces {
        if a.inherit_only || a.sid == CREATOR_OWNER || admin(&a.sid) {
            continue;
        }
        if a.mask & WRITE_RIGHTS != 0 {
            return Err(format!("{} can change {what}", a.sid));
        }
    }
    Ok(())
}

/// Both the folder and the file must pass: write access to the folder lets someone delete the file
/// and put another in its place.
pub fn check(reader: &dyn SecurityReader, file: &Path) -> Result<(), String> {
    let folder = file.parent().ok_or("the policy file has no folder")?;
    judge("the SprkNotes folder", &reader.read(folder)?)?;
    judge("policy.json", &reader.read(file)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    struct Fake(HashMap<PathBuf, SecurityInfo>);
    impl SecurityReader for Fake {
        fn read(&self, path: &Path) -> Result<SecurityInfo, String> {
            self.0.get(path).cloned().ok_or_else(|| "missing".into())
        }
    }

    fn ace(sid: &str, mask: u32) -> Ace {
        Ace {
            sid: sid.into(),
            mask,
            inherit_only: false,
        }
    }

    /// What the deployment script sets: SYSTEM and Administrators full control, Users read.
    fn locked() -> SecurityInfo {
        SecurityInfo {
            owner: ADMINISTRATORS.into(),
            dacl: Some(vec![
                ace(SYSTEM, 0x1F_01FF),
                ace(ADMINISTRATORS, 0x1F_01FF),
                ace("S-1-5-32-545", 0x12_00A9),
            ]),
        }
    }

    fn files(folder: SecurityInfo, file: SecurityInfo) -> (Fake, PathBuf) {
        let f = PathBuf::from("C:/ProgramData/SprkNotes/policy.json");
        let mut m = HashMap::new();
        m.insert(PathBuf::from("C:/ProgramData/SprkNotes"), folder);
        m.insert(f.clone(), file);
        (Fake(m), f)
    }

    #[test]
    fn the_deployment_scripts_permissions_are_trusted() {
        let (r, f) = files(locked(), locked());
        assert_eq!(check(&r, &f), Ok(()));
        // An inherit-only grant (passed to new children) and CREATOR OWNER grant nothing here.
        let mut folder = locked();
        if let Some(d) = folder.dacl.as_mut() {
            d.push(Ace {
                sid: "S-1-5-32-545".into(),
                mask: 0x1F_01FF,
                inherit_only: true,
            });
            d.push(ace(CREATOR_OWNER, 0x1000_0000));
        }
        let (r, f) = files(folder, locked());
        assert_eq!(check(&r, &f), Ok(()));
    }

    #[test]
    fn a_folder_or_file_users_can_change_is_ignored() {
        // A user made the folder first and owns it.
        let mut user_owned = locked();
        user_owned.owner = "S-1-5-21-1-2-3-1001".into();
        let (r, f) = files(user_owned, locked());
        assert!(check(&r, &f).is_err_and(|e| e.contains("owned by")));

        // ProgramData's default: Users may add files to subfolders.
        let mut open = locked();
        if let Some(d) = open.dacl.as_mut() {
            d.push(ace("S-1-5-32-545", 0x0000_0006));
        }
        let (r, f) = files(open, locked());
        assert!(check(&r, &f).is_err_and(|e| e.contains("S-1-5-32-545")));

        // The file itself writable by Authenticated Users.
        let mut file = locked();
        if let Some(d) = file.dacl.as_mut() {
            d.push(ace("S-1-5-11", 0x0013_01BF));
        }
        let (r, f) = files(locked(), file);
        assert!(check(&r, &f).is_err_and(|e| e.contains("policy.json")));

        // A null access list lets anyone in.
        let (r, f) = files(
            locked(),
            SecurityInfo {
                owner: SYSTEM.into(),
                dacl: None,
            },
        );
        assert!(check(&r, &f).is_err());
    }
}
