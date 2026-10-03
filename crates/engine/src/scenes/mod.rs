// SPDX-License-Identifier: GPL-3.0-or-later
//! Top-level flows.

pub mod browser;
pub mod demo;

use std::rc::Rc;

use openxeenng_formats::gamedata::GameData;

use crate::exec::Cx;

/// Entry point of the engine task.
pub async fn main_flow(cx: Cx, data: Option<Rc<GameData>>, message: Option<String>) {
    match data {
        Some(d) => browser::run(cx, d).await,
        None => demo::run(cx, message).await,
    }
}
