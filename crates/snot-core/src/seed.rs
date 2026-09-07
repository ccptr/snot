//! What a brand-new library starts with.
//!
//! Two quite different things live here. A real first launch lays down a
//! single welcome note — enough to explain the app to someone who has just
//! opened it, and nothing they then have to tidy up. The demo library is a
//! worked example of every feature, for testing and for screenshots; it is
//! opt-in and never appears in front of a real user.

use serde_json::{json, Value};

use crate::error::Result;
use crate::model::NotePatch;
use crate::store::Store;

/// Points are `[x, y, pressure]` in the 1000-unit-wide page space.
type Points = Vec<[f64; 3]>;

/// A drawn line is never quite straight; this keeps the sketch looking like a
/// hand made it rather than a plotter.
fn wobble(t: f64, amp: f64) -> f64 {
    ((t * 6.3).sin() * 0.6 + (t * 17.1).cos() * 0.4) * amp
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64, amp: f64) -> Points {
    let steps = 24;
    (0..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let nx = -(y1 - y0);
            let ny = x1 - x0;
            let len = (nx * nx + ny * ny).sqrt().max(1.0);
            let off = wobble(t + x0 * 0.01, amp);
            [
                x0 + (x1 - x0) * t + nx / len * off,
                y0 + (y1 - y0) * t + ny / len * off,
                0.5,
            ]
        })
        .collect()
}

fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64, amp: f64) -> Points {
    let steps = 44;
    (0..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let a = t * std::f64::consts::TAU;
            let r = 1.0 + wobble(t, amp) / 40.0;
            [cx + a.cos() * rx * r, cy + a.sin() * ry * r, 0.5]
        })
        .collect()
}

fn stroke(points: Points, color: &str, size: f64, tool: &str) -> Value {
    json!({ "points": points, "color": color, "size": size, "tool": tool })
}

fn para(text: &str) -> Value {
    json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
}

fn heading(level: u8, text: &str) -> Value {
    json!({
        "type": "heading",
        "attrs": { "level": level },
        "content": [{ "type": "text", "text": text }]
    })
}

fn task(checked: bool, text: &str) -> Value {
    json!({
        "type": "taskItem",
        "attrs": { "checked": checked },
        "content": [para(text)]
    })
}

fn item(text: &str) -> Value {
    json!({ "type": "listItem", "content": [para(text)] })
}

fn doc(blocks: Vec<Value>) -> Value {
    json!({ "type": "doc", "content": blocks })
}

/// One example note, described rather than built step by step.
struct Example {
    doc: Value,
    folder: Option<String>,
    tags: Vec<String>,
    ink: Option<Value>,
    pinned: bool,
    favorite: bool,
    trashed: bool,
    /// How long ago the note was written, in hours, so the list reads like a
    /// library someone has actually been keeping.
    age_hours: i64,
}

impl Example {
    fn new(doc: Value) -> Self {
        Example {
            doc,
            folder: None,
            tags: vec![],
            ink: None,
            pinned: false,
            favorite: false,
            trashed: false,
            age_hours: 1,
        }
    }

    fn folder(mut self, id: &str) -> Self {
        self.folder = Some(id.to_string());
        self
    }

    fn tag(mut self, id: &str) -> Self {
        self.tags.push(id.to_string());
        self
    }

    fn ink(mut self, strokes: Value) -> Self {
        self.ink = Some(strokes);
        self
    }

    fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }

    fn favorite(mut self) -> Self {
        self.favorite = true;
        self
    }

    fn trashed(mut self) -> Self {
        self.trashed = true;
        self
    }

    fn hours_ago(mut self, hours: i64) -> Self {
        self.age_hours = hours;
        self
    }
}

impl Store {
    /// Lays down starting content, once, in a library that has never held a
    /// note. `demo` chooses the full worked example over the welcome note.
    ///
    /// Does nothing if the library has been seeded before or already holds
    /// anything, so the notes cannot reappear after someone clears them out.
    pub fn seed_if_new(&self, demo: bool) -> Result<bool> {
        if self.meta("seeded")?.is_some() {
            return Ok(false);
        }
        let (notes, trashed, folders) = self.stats()?;
        if notes > 0 || trashed > 0 || folders > 0 {
            self.set_meta("seeded", "1")?;
            return Ok(false);
        }
        if demo {
            self.seed_demo_library()?;
        } else {
            self.seed_welcome()?;
        }
        self.set_meta("seeded", "1")?;
        Ok(true)
    }

    /// The one note a real first launch gets.
    fn seed_welcome(&self) -> Result<()> {
        self.add(Example::new(welcome_doc()).pinned())
    }

    fn add(&self, example: Example) -> Result<()> {
        let note = self.create_note(example.folder.as_deref(), Some(example.doc))?;
        let id = note.summary.id;
        self.update_note(
            &id,
            NotePatch {
                ink: example.ink,
                tags: Some(example.tags),
                pinned: Some(example.pinned),
                favorite: Some(example.favorite),
                ..Default::default()
            },
        )?;
        if example.trashed {
            self.trash_note(&id)?;
        }
        let when = crate::model::now_ms() - example.age_hours * 3_600_000;
        self.backdate(&id, when, when)?;
        Ok(())
    }

    fn seed_demo_library(&self) -> Result<()> {
        let work = self.create_folder("Work", None)?.id;
        let projects = self.create_folder("Projects", Some(&work))?.id;
        let meetings = self.create_folder("Meetings", Some(&work))?.id;
        let personal = self.create_folder("Personal", None)?.id;
        let recipes = self.create_folder("Recipes", Some(&personal))?.id;
        let house = self.create_folder("House", Some(&personal))?.id;
        let archive = self.create_folder("Archive", None)?.id;

        let urgent = self.ensure_tag("urgent")?.id;
        let ideas = self.ensure_tag("ideas")?.id;
        let cooking = self.ensure_tag("cooking")?.id;
        let travel = self.ensure_tag("travel")?.id;
        let reading = self.ensure_tag("reading")?.id;
        let money = self.ensure_tag("money")?.id;
        let home = self.ensure_tag("house")?.id;

        // ---------------------------------------------------------- welcome
        self.add(Example::new(welcome_doc()).pinned())?;

        // ------------------------------------------------------------- work
        self.add(
            Example::new(doc(vec![
                heading(2, "Monday standup"),
                json!({ "type": "taskList", "content": [
                    task(true, "Send the invoice"),
                    task(true, "Reply to the accessibility audit"),
                    task(false, "Draft the migration plan"),
                    task(false, "Book the room for Thursday"),
                ]}),
                json!({ "type": "paragraph", "content": [
                    { "type": "text", "text": "Blocked on " },
                    { "type": "text", "marks": [{ "type": "highlight", "attrs": { "color": "#fef08a" } }], "text": "the staging database" },
                    { "type": "text", "text": " — ask about it first." }
                ]}),
            ]))
            .folder(&meetings)
            .tag(&urgent)
            .pinned()
            .hours_ago(3),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Platform review"),
                para("Four of us, forty minutes, no slides. Better than the last one."),
                heading(3, "Decisions"),
                json!({ "type": "orderedList", "attrs": { "start": 1 }, "content": [
                    item("Keep the current queue. Revisit in the spring, not before."),
                    item("Split the ingest worker out so it can be scaled on its own."),
                    item("Nobody owns the flaky test suite. Someone has to."),
                ]}),
                json!({ "type": "blockquote", "content": [
                    para("\"If we cannot describe the failure, we cannot fix it.\" — worth remembering.")
                ]}),
                heading(3, "Follow-ups"),
                json!({ "type": "taskList", "content": [
                    task(false, "Write the ingest split up as a one-pager"),
                    task(false, "Ask about the test suite at the next standup"),
                ]}),
            ]))
            .folder(&meetings)
            .hours_ago(26),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Migration plan, first draft"),
                para("Move the note bodies out of the main table and into their own store. Do it in three passes so it can be stopped halfway."),
                json!({ "type": "orderedList", "attrs": { "start": 1 }, "content": [
                    item("Write to both, read from the old one. No behaviour change."),
                    item("Backfill in batches overnight. Compare checksums as it goes."),
                    item("Flip reads. Leave the old column in place for a week."),
                ]}),
                json!({ "type": "codeBlock", "attrs": { "language": "sql" }, "content": [
                    { "type": "text", "text": "SELECT COUNT(*) FROM notes\n WHERE body_checksum IS DISTINCT FROM legacy_checksum;" }
                ]}),
                json!({ "type": "paragraph", "content": [
                    { "type": "text", "text": "Rollback is " },
                    { "type": "text", "marks": [{ "type": "bold" }], "text": "flip reads back" },
                    { "type": "text", "text": ", nothing else. That is the whole point of the shape." }
                ]}),
            ]))
            .folder(&projects)
            .hours_ago(30),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Bug: search misses accented words"),
                para("Searching for \"cafe\" does not find \"café\". The tokeniser is folding the wrong way round."),
                json!({ "type": "codeBlock", "attrs": { "language": "" }, "content": [
                    { "type": "text", "text": "tokenize = \"unicode61 remove_diacritics 2\"" }
                ]}),
                json!({ "type": "bulletList", "content": [
                    item("Reproduced on a fresh library, so it is not an index that went stale"),
                    item("Only affects the body column, titles are fine"),
                    item("Rebuilding the index fixes it, which points at insert order"),
                ]}),
                json!({ "type": "paragraph", "content": [
                    { "type": "text", "marks": [{ "type": "strike" }], "text": "Probably the collation." },
                    { "type": "text", "text": " It was not the collation." }
                ]}),
            ]))
            .folder(&projects)
            .tag(&urgent)
            .hours_ago(50),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Invoice chase"),
                json!({ "type": "taskList", "content": [
                    task(true, "March — paid, eventually"),
                    task(true, "April — paid"),
                    task(false, "May — sent 3 weeks ago, chase on Friday"),
                    task(false, "June — not sent yet"),
                ]}),
                para("Their finance address changed in April and nobody said. Send to both from now on."),
            ]))
            .folder(&work)
            .tag(&money)
            .hours_ago(72),
        )?;

        // ------------------------------------------------------ house & ink
        self.add(
            Example::new(doc(vec![
                heading(2, "Shelf for the alcove"),
                para("Measure twice before cutting. The wall is not square at the bottom."),
                para(""),
                para(""),
                para(""),
                para(""),
                para(""),
                para("Oak, 18mm. Ask about offcuts."),
            ]))
            .folder(&house)
            .tag(&home)
            .ink(json!([
                stroke(line(180.0, 300.0, 700.0, 296.0, 2.5), "ink", 4.0, "pen"),
                stroke(line(700.0, 296.0, 704.0, 470.0, 2.5), "ink", 4.0, "pen"),
                stroke(line(704.0, 470.0, 184.0, 476.0, 2.5), "ink", 4.0, "pen"),
                stroke(line(184.0, 476.0, 180.0, 300.0, 2.5), "ink", 4.0, "pen"),
                stroke(line(180.0, 530.0, 700.0, 530.0, 1.2), "#dc2626", 2.0, "pen"),
                stroke(line(180.0, 530.0, 205.0, 516.0, 1.0), "#dc2626", 2.0, "pen"),
                stroke(line(180.0, 530.0, 205.0, 544.0, 1.0), "#dc2626", 2.0, "pen"),
                stroke(line(700.0, 530.0, 675.0, 516.0, 1.0), "#dc2626", 2.0, "pen"),
                stroke(line(700.0, 530.0, 675.0, 544.0, 1.0), "#dc2626", 2.0, "pen"),
                stroke(
                    ellipse(690.0, 470.0, 70.0, 46.0, 4.0),
                    "#dc2626",
                    3.0,
                    "pen"
                ),
            ]))
            .hours_ago(5),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Garden, rough plan"),
                para("South is to the right. The apple gets sun until about four."),
                para(""),
                para(""),
                para(""),
                para(""),
                para(""),
                para("Path in gravel, not slabs. Cheaper and it drains."),
            ]))
            .folder(&house)
            .tag(&home)
            .ink(json!([
                stroke(line(120.0, 260.0, 860.0, 254.0, 3.0), "ink", 3.0, "pen"),
                stroke(line(860.0, 254.0, 866.0, 560.0, 3.0), "ink", 3.0, "pen"),
                stroke(line(866.0, 560.0, 126.0, 566.0, 3.0), "ink", 3.0, "pen"),
                stroke(line(126.0, 566.0, 120.0, 260.0, 3.0), "ink", 3.0, "pen"),
                stroke(
                    ellipse(700.0, 350.0, 66.0, 60.0, 5.0),
                    "#16a34a",
                    4.0,
                    "pen"
                ),
                stroke(
                    ellipse(770.0, 470.0, 44.0, 40.0, 5.0),
                    "#16a34a",
                    4.0,
                    "pen"
                ),
                stroke(
                    line(150.0, 540.0, 620.0, 300.0, 6.0),
                    "#ca8a04",
                    7.0,
                    "marker"
                ),
                stroke(line(180.0, 300.0, 420.0, 300.0, 1.5), "#2563eb", 2.0, "pen"),
                stroke(line(180.0, 300.0, 180.0, 380.0, 1.5), "#2563eb", 2.0, "pen"),
            ]))
            .hours_ago(100),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Cellar damp — what the surveyor said"),
                para("Long version, because I will forget the detail by the time anyone quotes for it."),
                heading(3, "What it is not"),
                para("Not rising damp. He was quite firm about that, and slightly rude about the last person who said it was. The meter readings at skirting height were no higher than the middle of the wall, which is the tell."),
                heading(3, "What it probably is"),
                json!({ "type": "bulletList", "content": [
                    item("The ground outside sits about 200mm above the internal floor on the north side"),
                    item("The gully by the back door is blocked, so water pools against the wall"),
                    item("Original lime plaster was patched with gypsum, which traps the moisture behind it"),
                ]}),
                para("His view: fix the drainage first and live with it for a winter before doing anything expensive inside. Most of the trade will want to tank it immediately, which he described as \"selling a solution to the wrong problem\"."),
                heading(3, "Order of work"),
                json!({ "type": "orderedList", "attrs": { "start": 1 }, "content": [
                    item("Clear the gully and re-lay the fall away from the house"),
                    item("Take the gypsum patches off, back to the lime"),
                    item("Leave it alone until March"),
                    item("Re-measure. If it has dried, replaster in lime and stop there."),
                ]}),
                json!({ "type": "blockquote", "content": [
                    para("\"Buildings of this age are meant to breathe. Most damp is somebody having stopped them.\"")
                ]}),
                para("Quote for the drainage work only: he suggested budgeting eight hundred to a thousand, and warned that anyone quoting under three hundred has not looked at the fall properly."),
            ]))
            .folder(&house)
            .tag(&home)
            .hours_ago(200),
        )?;

        // ---------------------------------------------------------- recipes
        self.add(
            Example::new(doc(vec![
                heading(2, "Sourdough, slow overnight"),
                para("Makes one loaf. Start it after dinner."),
                json!({ "type": "orderedList", "attrs": { "start": 1 }, "content": [
                    item("Mix 400g strong white flour with 300g water. Rest 40 minutes."),
                    item("Add 80g starter and 9g salt. Fold every 30 minutes, four times."),
                    item("Shape, then into the fridge overnight."),
                    item("Bake at 240C, lid on for 20 minutes, lid off for 20 more."),
                ]}),
                json!({ "type": "blockquote", "content": [
                    para("If the crumb is tight the starter was not ready. Feed it twice next time.")
                ]}),
            ]))
            .folder(&recipes)
            .tag(&cooking)
            .favorite()
            .hours_ago(8),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Weeknight dal"),
                para("Twenty-five minutes, one pan, no soaking."),
                json!({ "type": "bulletList", "content": [
                    item("200g red lentils, rinsed until the water runs clear"),
                    item("Onion, four cloves of garlic, thumb of ginger"),
                    item("Two teaspoons cumin, one of turmeric, half of chilli"),
                    item("A tin of tomatoes and about 600ml water"),
                ]}),
                para("Fry the aromatics properly — six or seven minutes, not two. That is the whole difference."),
                json!({ "type": "paragraph", "content": [
                    { "type": "text", "text": "Finish with lemon. " },
                    { "type": "text", "marks": [{ "type": "italic" }], "text": "Always" },
                    { "type": "text", "text": " finish with lemon." }
                ]}),
            ]))
            .folder(&recipes)
            .tag(&cooking)
            .hours_ago(120),
        )?;

        // --------------------------------------------------------- personal
        self.add(
            Example::new(doc(vec![
                heading(2, "Lisbon, March"),
                para("Four nights. Flights booked, nothing else."),
                heading(3, "To book"),
                json!({ "type": "taskList", "content": [
                    task(true, "Flights"),
                    task(false, "Somewhere in Alfama, not the centre"),
                    task(false, "Train to Sintra — apparently go early or not at all"),
                    task(false, "Tell the bank about the card"),
                ]}),
                heading(3, "Told about"),
                json!({ "type": "bulletList", "content": [
                    item("The tiled market hall, mornings only"),
                    item("A tram that is not the tourist one and costs a third as much"),
                    item("Custard tarts: the queue is worth it once, not twice"),
                ]}),
            ]))
            .folder(&personal)
            .tag(&travel)
            .hours_ago(20),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Reading list"),
                json!({ "type": "bulletList", "content": [
                    item("Seeing Like a State — half read, keeps being put down"),
                    item("The Making of the Atomic Bomb — everyone says it is worth the length"),
                    item("Something fiction, for once"),
                ]}),
                para("Finished this year: four. Bought this year: nineteen. This is a known problem."),
            ]))
            .tag(&reading)
            .hours_ago(40),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Half-formed ideas"),
                json!({ "type": "bulletList", "content": [
                    item("A bike light that gets brighter when it hears a car"),
                    item("Teach the kids to solder — start with the blinking badge"),
                    item("Something about the allotment waiting list being public"),
                    item("An app that only lets you write one note a day"),
                ]}),
                json!({ "type": "codeBlock", "attrs": { "language": "" }, "content": [
                    { "type": "text", "text": "rsync -a --delete ~/notes/ backup:notes/" }
                ]}),
            ]))
            .tag(&ideas)
            .hours_ago(12),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Guitar, this month"),
                json!({ "type": "taskList", "content": [
                    task(true, "Barre chords without the buzz on the B string"),
                    task(false, "Blackbird, the middle section"),
                    task(false, "Learn the notes on the fifth and sixth strings properly"),
                ]}),
                para("Fifteen minutes daily beats two hours on a Sunday. Proven repeatedly, ignored repeatedly."),
            ]))
            .folder(&personal)
            .hours_ago(60),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Car service history"),
                json!({ "type": "orderedList", "attrs": { "start": 1 }, "content": [
                    item("2023 — full service, rear pads, 62,400 miles"),
                    item("2024 — oil and filter only, 71,900 miles"),
                    item("2025 — MOT advisory on the front tyres, 78,300 miles"),
                    item("2026 — cambelt due, budget for it"),
                ]}),
                para("Garage on the industrial estate, not the main dealer. Ask for Dave."),
            ]))
            .folder(&archive)
            .hours_ago(300),
        )?;

        // ------------------------------------------------------------ trash
        self.add(
            Example::new(doc(vec![
                heading(2, "Shopping, last Tuesday"),
                json!({ "type": "taskList", "content": [
                    task(true, "Milk"),
                    task(true, "Coffee"),
                    task(true, "Washing-up liquid"),
                ]}),
                para("Deleted, but not gone. Restore it from the trash."),
            ]))
            .trashed()
            .hours_ago(90),
        )?;

        self.add(
            Example::new(doc(vec![
                heading(2, "Gym plan that lasted nine days"),
                json!({ "type": "bulletList", "content": [
                    item("Monday, Wednesday, Friday, no excuses"),
                    item("Squats, deadlift, press. Nothing clever."),
                ]}),
                para("It was a good plan. That was never the problem."),
            ]))
            .trashed()
            .hours_ago(400),
        )?;

        Ok(())
    }
}

/// The note someone sees the first time they open the app.
fn welcome_doc() -> Value {
    doc(vec![
        heading(1, "Welcome to Snot"),
        json!({ "type": "paragraph", "content": [
            { "type": "text", "text": "Notes that stay " },
            { "type": "text", "marks": [{ "type": "bold" }], "text": "yours" },
            { "type": "text", "text": ". Everything here is a file on your own disk — no account, no cloud, no lock-in." }
        ]}),
        heading(2, "Try these"),
        json!({ "type": "taskList", "content": [
            task(true, "Read this note"),
            task(false, "Press the pen in the toolbar and scribble anywhere on this page"),
            task(false, "Import a PDF with the arrow button, then write on it"),
            task(false, "Search for \"sourdough\" — it looks inside every note"),
            task(false, "Drag a note from the list onto a folder to file it"),
            task(false, "Open the trash and restore something"),
        ]}),
        heading(2, "What it does"),
        json!({ "type": "bulletList", "content": [
            item("Rich text: headings, checklists, quotes, code and highlighting"),
            item("Handwriting over the whole page, not stuck in a box"),
            item("Folders, tags, favourites, pinning and a trash you can undo from"),
            item("Full-text search that highlights what it matched"),
        ]}),
        json!({ "type": "blockquote", "content": [
            para("Deleted notes go to the trash and stay there until you empty it. Nothing is destroyed behind your back.")
        ]}),
        json!({ "type": "horizontalRule" }),
        json!({ "type": "paragraph", "content": [
            { "type": "text", "text": "Your library lives in a plain SQLite file. Point the app somewhere else with " },
            { "type": "text", "marks": [{ "type": "code" }], "text": "SNOT_LIBRARY" },
            { "type": "text", "text": "." }
        ]}),
    ])
}
