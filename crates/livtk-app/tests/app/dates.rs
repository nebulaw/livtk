//! The date picker's dates: the grid, the quick picks, a date typed in words, the keys.

use jiff::civil::{Date, date};
use livtk_app::model::dates::{Month, Quick, Range, Step, WeekStart, parse, short, step};

/// A Tuesday.
const TODAY: Date = date(2026, 10, 27);

fn read(text: &str) -> Date {
    parse(text, TODAY, WeekStart::Monday).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

#[test]
fn a_months_grid_keeps_every_column_one_weekday_and_holds_each_day_once() {
    for (y, m) in [
        (2026, 2),
        (2028, 2),
        (2100, 2),
        (2026, 12),
        (2027, 1),
        (2026, 10),
    ] {
        for start in [WeekStart::Monday, WeekStart::Sunday] {
            let month = Month::of(date(y, m, 1));
            let grid = month.grid(start);
            let days: Vec<Date> = grid.iter().flatten().flatten().copied().collect();
            assert_eq!(days.first(), Some(&month.first()), "{y}-{m}");
            assert_eq!(days.last(), Some(&month.last()), "{y}-{m}");
            assert_eq!(
                days.len(),
                usize::try_from(month.last().day()).unwrap_or(0),
                "every day once"
            );
            for week in &grid {
                for (col, d) in week.iter().enumerate() {
                    if let Some(d) = d {
                        let want = match start {
                            WeekStart::Monday => d.weekday().to_monday_zero_offset(),
                            WeekStart::Sunday => d.weekday().to_sunday_zero_offset(),
                        };
                        assert_eq!(usize::try_from(want).unwrap_or(9), col, "{d}");
                    }
                }
            }
        }
    }
    // Leap years: 2028 has a Feb 29, 2100 has not.
    assert_eq!(Month::of(date(2028, 2, 1)).last(), date(2028, 2, 29));
    assert_eq!(Month::of(date(2100, 2, 1)).last(), date(2100, 2, 28));
}

#[test]
fn months_step_across_the_year_and_keep_to_their_first_day() {
    let oct = Month::of(TODAY);
    assert_eq!(oct.first(), date(2026, 10, 1));
    assert_eq!(oct.plus(3).first(), date(2027, 1, 1));
    assert_eq!(oct.plus(-10).first(), date(2025, 12, 1));
    assert_eq!(oct.title(), "October 2026");
}

#[test]
fn the_quick_picks_land_where_they_say() {
    let mon = WeekStart::Monday;
    assert_eq!(Quick::EndOfWeek.from(TODAY, mon), date(2026, 11, 1));
    assert_eq!(
        Quick::EndOfWeek.from(TODAY, WeekStart::Sunday),
        date(2026, 10, 31)
    );
    // On the week's last day, it is today.
    assert_eq!(
        Quick::EndOfWeek.from(date(2026, 11, 1), mon),
        date(2026, 11, 1)
    );
    assert_eq!(Quick::EndOfMonth.from(TODAY, mon), date(2026, 10, 31));
    assert_eq!(
        Quick::EndOfMonth.from(date(2028, 2, 3), mon),
        date(2028, 2, 29)
    );
    assert_eq!(Quick::EndOfQuarter.from(TODAY, mon), date(2026, 12, 31));
    assert_eq!(
        Quick::EndOfQuarter.from(date(2026, 1, 1), mon),
        date(2026, 3, 31)
    );
    assert_eq!(Quick::EndOfYear.from(TODAY, mon), date(2026, 12, 31));
}

#[test]
fn dates_typed_in_words_read_as_people_mean_them() {
    for (text, want) in [
        ("2026-12-15", date(2026, 12, 15)),
        ("  2027-01-02 ", date(2027, 1, 2)),
        ("today", TODAY),
        ("Tomorrow", date(2026, 10, 28)),
        ("yesterday", date(2026, 10, 26)),
        ("Dec 15", date(2026, 12, 15)),
        ("15 december", date(2026, 12, 15)),
        ("dec. 15, 2027", date(2027, 12, 15)),
        ("sept 3", date(2027, 9, 3)),
        // No year: the next one to come; today counts.
        ("oct 27", TODAY),
        ("oct 26", date(2027, 10, 26)),
        ("jan 1st", date(2027, 1, 1)),
        // Feb 29 waits for the next leap year.
        ("feb 29", date(2028, 2, 29)),
        // A weekday: the next one after today, never today.
        ("friday", date(2026, 10, 30)),
        ("next tue", date(2026, 11, 3)),
        ("in 3 days", date(2026, 10, 30)),
        ("in 2 weeks", date(2026, 11, 10)),
        ("in a month", date(2026, 11, 27)),
        ("in 1 year", date(2027, 10, 27)),
        ("eom", date(2026, 10, 31)),
        ("end of the quarter", date(2026, 12, 31)),
        ("eow", date(2026, 11, 1)),
    ] {
        assert_eq!(read(text), want, "{text}");
    }
}

#[test]
fn text_that_is_no_date_is_said_back_never_guessed() {
    for text in [
        "",
        "   ",
        "soon",
        "12/15",
        "15.12",
        "feb 30 2026",
        "in x days",
        "in 2 fortnights",
        "ma 3",
        "dec 32",
    ] {
        let e = parse(text, TODAY, WeekStart::Monday).expect_err(text);
        assert!(e.0.contains("Can't read"), "{text}: {e:?}");
    }
}

#[test]
fn the_keys_move_a_day_within_its_range() {
    let open = Range::default();
    let mon = WeekStart::Monday;
    assert_eq!(step(TODAY, Step::Day(1), mon, open), date(2026, 10, 28));
    assert_eq!(step(TODAY, Step::Week(-1), mon, open), date(2026, 10, 20));
    // A month on from Jan 31 is Feb's last day, never March.
    assert_eq!(
        step(date(2027, 1, 31), Step::Month(1), mon, open),
        date(2027, 2, 28)
    );
    assert_eq!(step(TODAY, Step::WeekStart, mon, open), date(2026, 10, 26));
    assert_eq!(step(TODAY, Step::WeekEnd, mon, open), date(2026, 11, 1));
    assert_eq!(
        step(TODAY, Step::WeekStart, WeekStart::Sunday, open),
        date(2026, 10, 25)
    );
    // Past the range's edge, it stops at the edge.
    let range = Range {
        min: Some(TODAY),
        max: Some(date(2026, 11, 5)),
    };
    assert_eq!(step(TODAY, Step::Day(-1), mon, range), TODAY);
    assert_eq!(step(TODAY, Step::Month(1), mon, range), date(2026, 11, 5));
    assert!(range.contains(TODAY) && !range.contains(date(2026, 11, 6)));
}

#[test]
fn a_chip_shows_a_date_short_with_its_year_only_when_another() {
    assert_eq!(short(date(2026, 12, 15), TODAY), "Dec 15");
    assert_eq!(short(date(2027, 1, 2), TODAY), "Jan 2, 2027");
}
