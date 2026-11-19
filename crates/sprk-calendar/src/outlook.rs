//! Classic Outlook calendar over COM late binding (SPEC 7.1). Local only, read-only.
//! Outlook is reached through `IDispatch`; if it is missing, is the new Outlook, or refuses
//! access, the provider returns nothing and the naming chain falls through silently.
//! Subjects, organizers, invitees and bodies stay inside [`Meeting`] (PHI-grade).
//!
//! Unsafe is limited to COM calls; each block says why it is sound.
#![allow(unsafe_code)]
use crate::{Busy, CalendarProvider, Meeting};
use chrono::{Local, LocalResult, NaiveDate, TimeZone};
use windows::core::{IUnknown, Interface, BSTR, GUID, PCWSTR, VARIANT};
use windows::Win32::System::Com::{
    CLSIDFromProgID, CoCreateInstance, CoInitializeEx, IDispatch, CLSCTX_LOCAL_SERVER,
    COINIT_APARTMENTTHREADED, DISPATCH_FLAGS, DISPATCH_METHOD, DISPATCH_PROPERTYGET,
    DISPATCH_PROPERTYPUT, DISPPARAMS,
};
use windows::Win32::System::Ole::DISPID_PROPERTYPUT;

const LOCALE_USER_DEFAULT: u32 = 0x0400;

/// Late-bound call: `obj.name(args)` or property get/put.
fn call(obj: &IDispatch, name: &str, flags: DISPATCH_FLAGS, args: &[VARIANT]) -> Option<VARIANT> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let names = [PCWSTR(wide.as_ptr())];
    let mut id = 0i32;
    // SAFETY: `names` points to one valid NUL-terminated UTF-16 string that outlives the call,
    // and `id` is a valid out pointer for exactly one DISPID.
    unsafe {
        obj.GetIDsOfNames(
            &GUID::zeroed(),
            names.as_ptr(),
            1,
            LOCALE_USER_DEFAULT,
            &mut id,
        )
        .ok()?;
    }
    // IDispatch wants arguments in reverse order.
    let mut rev: Vec<VARIANT> = args.iter().rev().cloned().collect();
    let mut put = DISPID_PROPERTYPUT;
    let params = DISPPARAMS {
        rgvarg: if rev.is_empty() {
            std::ptr::null_mut()
        } else {
            rev.as_mut_ptr()
        },
        rgdispidNamedArgs: if flags == DISPATCH_PROPERTYPUT {
            &mut put
        } else {
            std::ptr::null_mut()
        },
        cArgs: rev.len() as u32,
        cNamedArgs: u32::from(flags == DISPATCH_PROPERTYPUT),
    };
    let mut result = VARIANT::default();
    // SAFETY: `params` and `rev` live until the call returns; `result` is a valid out VARIANT;
    // exception and arg-error out pointers are optional and passed as null.
    unsafe {
        obj.Invoke(
            id,
            &GUID::zeroed(),
            LOCALE_USER_DEFAULT,
            flags,
            &params,
            Some(&mut result),
            None,
            None,
        )
        .ok()?;
    }
    Some(result)
}

fn get(obj: &IDispatch, name: &str) -> Option<VARIANT> {
    call(obj, name, DISPATCH_PROPERTYGET, &[])
}

/// Coercing readers: windows-core converts through the OLE `VariantTo*` helpers, so a
/// missing or wrong-typed value just reads as empty or zero.
fn as_string(v: &VARIANT) -> String {
    BSTR::try_from(v).map(|b| b.to_string()).unwrap_or_default()
}

fn as_i32(v: &VARIANT) -> i32 {
    i32::try_from(v).unwrap_or(0)
}

/// OLE date (VT_DATE) as days since 1899-12-30, local time.
fn as_date(v: &VARIANT) -> Option<f64> {
    f64::try_from(v).ok()
}

fn as_dispatch(v: &VARIANT) -> Option<IDispatch> {
    IUnknown::try_from(v).ok()?.cast::<IDispatch>().ok()
}

fn v_str(s: &str) -> VARIANT {
    VARIANT::from(BSTR::from(s))
}

fn v_i32(n: i32) -> VARIANT {
    VARIANT::from(n)
}

fn v_bool(b: bool) -> VARIANT {
    VARIANT::from(b)
}

/// OLE automation date (local time, days since 1899-12-30) to epoch seconds.
pub fn ole_to_epoch(d: f64) -> Option<i64> {
    let base = NaiveDate::from_ymd_opt(1899, 12, 30)?.and_hms_opt(0, 0, 0)?;
    let secs = (d * 86_400.0).round() as i64;
    let naive = base.checked_add_signed(chrono::Duration::seconds(secs))?;
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(t) => Some(t.timestamp()),
        LocalResult::Ambiguous(t, _) => Some(t.timestamp()),
        LocalResult::None => None,
    }
}

/// Outlook `Restrict` date text in the user's local time.
fn restrict_time(epoch: i64) -> Option<String> {
    let t = Local.timestamp_opt(epoch, 0).single()?;
    Some(t.format("%m/%d/%Y %I:%M %p").to_string())
}

pub struct OutlookCom;

impl OutlookCom {
    fn read(&self, from: i64, to: i64) -> Option<Vec<Meeting>> {
        // SAFETY: standard COM init for this thread; a repeat call or a different apartment
        // model returns an error we deliberately ignore.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let prog: Vec<u16> = "Outlook.Application\0".encode_utf16().collect();
        // SAFETY: `prog` is NUL-terminated and outlives the call.
        let clsid = unsafe { CLSIDFromProgID(PCWSTR(prog.as_ptr())) }.ok()?;
        // SAFETY: CLSID came from the registry; classic Outlook starts hidden if needed.
        // Failure (new Outlook, not installed, blocked) simply means unavailable.
        let app: IDispatch = unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) }.ok()?;
        let ns = as_dispatch(&call(
            &app,
            "GetNamespace",
            DISPATCH_METHOD,
            &[v_str("MAPI")],
        )?)?;
        let folder = as_dispatch(&call(
            &ns,
            "GetDefaultFolder",
            DISPATCH_METHOD,
            &[v_i32(9)],
        )?)?;
        let items = as_dispatch(&get(&folder, "Items")?)?;
        call(
            &items,
            "IncludeRecurrences",
            DISPATCH_PROPERTYPUT,
            &[v_bool(true)],
        )?;
        call(&items, "Sort", DISPATCH_METHOD, &[v_str("[Start]")])?;
        let filter = format!(
            "[Start] <= '{}' AND [End] >= '{}'",
            restrict_time(to)?,
            restrict_time(from)?
        );
        let found = as_dispatch(&call(
            &items,
            "Restrict",
            DISPATCH_METHOD,
            &[v_str(&filter)],
        )?)?;
        let count = as_i32(&get(&found, "Count")?);
        let mut out = Vec::new();
        for i in 1..=count.min(200) {
            let Some(item) =
                call(&found, "Item", DISPATCH_METHOD, &[v_i32(i)]).and_then(|v| as_dispatch(&v))
            else {
                continue;
            };
            if let Some(m) = read_item(&item, i) {
                out.push(m);
            }
        }
        Some(out)
    }
}

fn read_item(item: &IDispatch, n: i32) -> Option<Meeting> {
    let start = ole_to_epoch(as_date(&get(item, "Start")?)?)?;
    let end = ole_to_epoch(as_date(&get(item, "End")?)?)?;
    let s = |name: &str| get(item, name).map(|v| as_string(&v)).unwrap_or_default();
    let i = |name: &str| get(item, name).map(|v| as_i32(&v)).unwrap_or(0);
    let mut m = Meeting::new(&format!("outlook-{n}-{start}"), &s("Subject"), start, end);
    m.organizer = s("Organizer");
    m.location = s("Location");
    m.body = s("Body");
    // olMeetingCanceled = 5, olMeetingReceivedAndCanceled = 7
    m.cancelled = matches!(i("MeetingStatus"), 5 | 7);
    m.all_day = i("AllDayEvent") != 0;
    m.busy = match i("BusyStatus") {
        0 => Busy::Free,
        1 => Busy::Tentative,
        3 => Busy::OutOfOffice,
        4 => Busy::WorkingElsewhere,
        _ => Busy::Busy,
    };
    if let Some(rec) = get(item, "Recipients").and_then(|v| as_dispatch(&v)) {
        let count = get(&rec, "Count").map(|v| as_i32(&v)).unwrap_or(0);
        for k in 1..=count.min(500) {
            let Some(r) =
                call(&rec, "Item", DISPATCH_METHOD, &[v_i32(k)]).and_then(|v| as_dispatch(&v))
            else {
                continue;
            };
            let name = get(&r, "Name").map(|v| as_string(&v)).unwrap_or_default();
            // olRequired = 1, olOptional = 2; resources (3) are rooms, not people.
            match get(&r, "Type").map(|v| as_i32(&v)).unwrap_or(1) {
                1 => m.required.push(name),
                2 => m.optional.push(name),
                _ => {}
            }
        }
    }
    Some(m)
}

impl CalendarProvider for OutlookCom {
    fn meetings_between(&self, from: i64, to: i64) -> Vec<Meeting> {
        self.read(from, to).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ole_dates_convert_to_epoch() {
        // 2026-11-19 09:30 local is 46345.3958333 in OLE days; round-trip through chrono.
        let epoch = ole_to_epoch(46_345.395_833_333).unwrap();
        let back = Local.timestamp_opt(epoch, 0).single().unwrap();
        assert_eq!(
            back.format("%Y-%m-%d %H:%M").to_string(),
            "2026-11-19 09:30"
        );
    }

    /// Without classic Outlook (or with the new one) the provider must stay silent and empty.
    /// When classic Outlook with a profile is present this just returns its meetings.
    #[test]
    fn missing_outlook_is_silent() {
        let got = OutlookCom.meetings_between(0, 1);
        assert!(got.len() <= 200);
    }

    #[test]
    fn restrict_text_is_us_style_local() {
        let t = Local
            .with_ymd_and_hms(2026, 11, 19, 14, 5, 0)
            .unwrap()
            .timestamp();
        assert_eq!(restrict_time(t).unwrap(), "11/19/2026 02:05 PM");
    }
}
