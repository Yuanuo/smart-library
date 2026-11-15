//! The diary: an internal library (`diaries` / `我的日记`) whose notes are kept as
//! `年 / 月 / 日` (`20260000` › `20261100` › `20261108`). A diary is an ordinary
//! Markdown note; only its place and its IDs follow the date. The date comes
//! from the caller (the local date of the user's machine), so this module has
//! no clock.

/// The fixed ID of the diary library.
pub const LIBRARY_ID: &str = "diaries";

/// The default title of the diary library.
pub const LIBRARY: &str = "我的日记";

fn parse_date(date: &str) -> Result<(i64, i64, i64), String> {
    let bad = || format!("日期不对：{}（应为 YYYY-MM-DD）", date);
    let mut it = date.split('-');
    let (y, m, d) = (it.next().ok_or_else(bad)?, it.next().ok_or_else(bad)?, it.next().ok_or_else(bad)?);
    if it.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return Err(bad());
    }
    let num = |s: &str| s.parse::<i64>().map_err(|_| bad());
    let (yn, mn, dn) = (num(y)?, num(m)?, num(d)?);
    if yn <= 0 || !(1..=12).contains(&mn) || !(1..=31).contains(&dn) {
        return Err(bad());
    }
    Ok((yn, mn, dn))
}

/// The three default titles for a date `YYYY-MM-DD`: the year, the month and the
/// day's note (`["2026年", "11月", "2026-11-08 日记"]`).
pub fn path_of(date: &str) -> Result<[String; 3], String> {
    let (yn, mn, _) = parse_date(date)?;
    Ok([format!("{}年", yn), format!("{}月", mn), format!("{} 日记", date)])
}

/// The three fixed numeric node IDs for a date `YYYY-MM-DD`:
/// `[20260000, 20261100, 20261108]`.
pub fn ids_of(date: &str) -> Result<[i64; 3], String> {
    let (yn, mn, dn) = parse_date(date)?;
    let y = yn * 10000;
    let ym = y + mn * 100;
    let ymd = ym + dn;
    Ok([y, ym, ymd])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_ids_follow_the_date() {
        assert_eq!(path_of("2026-11-08").unwrap(), ["2026年", "11月", "2026-11-08 日记"]);
        assert_eq!(path_of("2026-01-31").unwrap()[1], "1月");
        assert_eq!(ids_of("2026-11-08").unwrap(), [20260000, 20261100, 20261108]);
        assert_eq!(ids_of("2026-01-31").unwrap(), [20260000, 20260100, 20260131]);
        for bad in ["2026-13-01", "2026-1-1", "20261108", "abc", "2026-11-08-1", "0000-01-01", ""] {
            assert!(path_of(bad).is_err(), "{}", bad);
            assert!(ids_of(bad).is_err(), "{}", bad);
        }
    }
}
