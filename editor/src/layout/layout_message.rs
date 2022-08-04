use super::widgets::Layout;
use crate::message_prelude::*;

use serde::{Deserialize, Serialize};

#[remain::sorted]
#[impl_message(Message, Layout)]
#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub enum LayoutMessage {
	RefreshLayout { layout_target: LayoutTarget },
	SendLayout { layout: Layout, layout_target: LayoutTarget },
	UpdateLayout { layout_target: LayoutTarget, widget_id: u64, value: serde_json::Value },
}

#[remain::sorted]
#[derive(PartialEq, Clone, Debug, Hash, Eq, Copy, Serialize, Deserialize)]
#[repr(u8)]
pub enum LayoutTarget {
	DialogDetails,
	DocumentBar,
	DocumentMode,
	LayerTreeOptions,
	MenuBar,
	PropertiesOptions,
	PropertiesSections,
	ToolOptions,
	ToolShelf,
	WorkingColors,

	// KEEP THIS ENUM LAST
	// This is a marker that is used to define an array that is used to hold widgets
	#[remain::unsorted]
	LayoutTargetLength,
}