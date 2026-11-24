//! Windows default audio endpoints by role. A Bluetooth headset shows up as two render endpoints
//! (stereo for music, hands-free for calls) and a call app uses the *communications* default,
//! which is often not the console default that cpal returns. Meeting audio and the headset mic
//! only exist on the communications endpoints, so those are what a transcript of a meeting needs.
//! Read-only: nothing here changes any Windows setting.
#![allow(unsafe_code)]
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

/// Which default to ask Windows for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Console,
    Communications,
}

/// Friendly name (the same text cpal reports) of the default endpoint for `capture` (true:
/// microphones, false: speakers) in `role`. None when there is none or Windows refuses.
pub fn default_name(capture: bool, role: Role) -> Option<String> {
    // SAFETY: COM is initialized for this thread (an already-initialized apartment returns an
    // error we ignore). Every interface is an owned `windows` wrapper released on drop.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let flow = if capture { eCapture } else { eRender };
        let erole = match role {
            Role::Console => eConsole,
            Role::Communications => eCommunications,
        };
        let dev = en.GetDefaultAudioEndpoint(flow, erole).ok()?;
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let name = value.to_string();
        (!name.is_empty()).then_some(name)
    }
}
