//! `plan.json`: the open world's plan, written by hand.
//!
//! Coordinates are `[lat, lon]` in degrees, the order Google Maps copies
//! them in, so a place can be pasted straight from a map. Every other field
//! is described in `docs/vision/atlas.md` section 3.

use crate::geo::{LatLon, num, text};
use serde_json::Value;

/// A named point: a junction, a viewpoint, a start line, a landmark.
#[derive(Clone, Debug)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub at: LatLon,
    pub note: Option<String>,
}

/// An area of the world with one character.
#[derive(Clone, Debug)]
pub struct Region {
    pub id: String,
    pub name: String,
    /// What it should feel like.
    pub character: String,
    /// The real place it compresses.
    pub real: Option<String>,
    /// The scenery generators (existing levels' kits) it would draw on.
    pub kits: Vec<String>,
    /// Built, planned or later.
    pub stage: String,
    pub ring: Vec<LatLon>,
    pub note: Option<String>,
}

/// A drive along real roads from one place to another.
#[derive(Clone, Debug)]
pub struct Route {
    pub id: String,
    pub name: String,
    /// The route numbers it may follow ("1", "92"); empty for any road.
    pub roads: Vec<String>,
    /// Road names it may follow ("Tunitas Creek Road"), with `roads`.
    pub names: Vec<String>,
    /// Road classes it may follow when `roads` and `names` are empty.
    pub classes: Vec<String>,
    pub from: String,
    pub via: Vec<String>,
    pub to: String,
    /// Game metres per real metre, when not the plan's.
    pub scale: Option<f64>,
    pub note: Option<String>,
}

/// Something to do: a race, a time trial, a cruise.
#[derive(Clone, Debug)]
pub struct Event {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// The route whose roads it follows.
    pub route: String,
    pub from: String,
    pub to: String,
    /// The light: "night into dawn", "golden hour".
    pub light: Option<String>,
    pub note: Option<String>,
}

/// A loop of routes, end to end.
#[derive(Clone, Debug)]
pub struct Circuit {
    pub id: String,
    pub name: String,
    pub routes: Vec<String>,
    pub note: Option<String>,
}

/// What becomes of an existing level in the world.
#[derive(Clone, Debug)]
pub struct Remix {
    pub level: String,
    pub now: String,
    pub becomes: String,
    pub region: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub world: String,
    /// Game metres per real metre (the compression; routes may override).
    pub scale: f64,
    pub places: Vec<Place>,
    pub regions: Vec<Region>,
    pub routes: Vec<Route>,
    pub circuits: Vec<Circuit>,
    pub events: Vec<Event>,
    pub remix: Vec<Remix>,
}

fn latlon(v: &Value) -> Result<LatLon, String> {
    Ok(LatLon {
        lat: v[0].as_f64().ok_or("expected [lat, lon]")?,
        lon: v[1].as_f64().ok_or("expected [lat, lon]")?,
    })
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn req(v: &Value, k: &str, what: &str) -> Result<String, String> {
    text(v, k).ok_or_else(|| format!("{what}: `{k}` missing"))
}

impl Plan {
    pub fn from_json(bytes: &[u8]) -> Result<Plan, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("plan.json: {e}"))?;
        let arr = |k: &str| v[k].as_array().cloned().unwrap_or_default();
        let mut places = Vec::new();
        for p in arr("places") {
            let id = req(&p, "id", "a place")?;
            places.push(Place {
                name: text(&p, "name").unwrap_or_else(|| id.clone()),
                kind: text(&p, "kind").unwrap_or_default(),
                at: latlon(&p["at"]).map_err(|e| format!("place `{id}`: {e}"))?,
                note: text(&p, "note"),
                id,
            });
        }
        let mut regions = Vec::new();
        for r in arr("regions") {
            let id = req(&r, "id", "a region")?;
            let ring = r["ring"]
                .as_array()
                .map(|a| a.iter().map(latlon).collect::<Result<Vec<_>, _>>())
                .transpose()
                .map_err(|e| format!("region `{id}`: {e}"))?
                .unwrap_or_default();
            regions.push(Region {
                name: text(&r, "name").unwrap_or_else(|| id.clone()),
                character: text(&r, "character").unwrap_or_default(),
                real: text(&r, "real"),
                kits: strings(&r["kits"]),
                stage: text(&r, "stage").unwrap_or_else(|| "planned".into()),
                ring,
                note: text(&r, "note"),
                id,
            });
        }
        let mut routes = Vec::new();
        for r in arr("routes") {
            let id = req(&r, "id", "a route")?;
            routes.push(Route {
                name: text(&r, "name").unwrap_or_else(|| id.clone()),
                roads: strings(&r["roads"]),
                names: strings(&r["names"]),
                classes: strings(&r["classes"]),
                from: req(&r, "from", &format!("route `{id}`"))?,
                via: strings(&r["via"]),
                to: req(&r, "to", &format!("route `{id}`"))?,
                scale: r["scale"].as_f64(),
                note: text(&r, "note"),
                id,
            });
        }
        let mut events = Vec::new();
        for e in arr("events") {
            let id = req(&e, "id", "an event")?;
            let what = format!("event `{id}`");
            events.push(Event {
                name: text(&e, "name").unwrap_or_else(|| id.clone()),
                kind: text(&e, "kind").unwrap_or_else(|| "race".into()),
                route: req(&e, "route", &what)?,
                from: req(&e, "from", &what)?,
                to: req(&e, "to", &what)?,
                light: text(&e, "light"),
                note: text(&e, "note"),
                id,
            });
        }
        let circuits = arr("circuits")
            .iter()
            .map(|c| {
                let id = req(c, "id", "a circuit")?;
                Ok(Circuit {
                    name: text(c, "name").unwrap_or_else(|| id.clone()),
                    routes: strings(&c["routes"]),
                    note: text(c, "note"),
                    id,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let remix = arr("remix")
            .iter()
            .map(|m| {
                Ok(Remix {
                    level: req(m, "level", "a remix")?,
                    now: text(m, "now").unwrap_or_default(),
                    becomes: text(m, "becomes").unwrap_or_default(),
                    region: text(m, "region"),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let plan = Plan {
            world: text(&v, "world").unwrap_or_default(),
            scale: num(&v, "scale").unwrap_or(1.0),
            places,
            regions,
            routes,
            circuits,
            events,
            remix,
        };
        plan.check_refs()?;
        Ok(plan)
    }

    /// Every id a route, event or circuit names exists, and ids are unique.
    fn check_refs(&self) -> Result<(), String> {
        let place = |id: &str, by: &str| {
            if self.places.iter().any(|p| p.id == id) {
                Ok(())
            } else {
                Err(format!("{by} names place `{id}`, which the plan lacks"))
            }
        };
        let route = |id: &str, by: &str| {
            if self.routes.iter().any(|r| r.id == id) {
                Ok(())
            } else {
                Err(format!("{by} names route `{id}`, which the plan lacks"))
            }
        };
        for r in &self.routes {
            let by = format!("route `{}`", r.id);
            place(&r.from, &by)?;
            place(&r.to, &by)?;
            for v in &r.via {
                place(v, &by)?;
            }
        }
        for e in &self.events {
            let by = format!("event `{}`", e.id);
            route(&e.route, &by)?;
            place(&e.from, &by)?;
            place(&e.to, &by)?;
        }
        for c in &self.circuits {
            for r in &c.routes {
                route(r, &format!("circuit `{}`", c.id))?;
            }
        }
        let mut ids: Vec<&str> = self
            .places
            .iter()
            .map(|p| p.id.as_str())
            .chain(self.regions.iter().map(|r| r.id.as_str()))
            .chain(self.routes.iter().map(|r| r.id.as_str()))
            .chain(self.events.iter().map(|e| e.id.as_str()))
            .chain(self.circuits.iter().map(|c| c.id.as_str()))
            .collect();
        ids.sort();
        if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
            return Err(format!("plan.json: the id `{}` is used twice", w[0]));
        }
        Ok(())
    }

    pub fn route(&self, id: &str) -> Option<&Route> {
        self.routes.iter().find(|r| r.id == id)
    }
}
