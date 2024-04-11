use dyn_any::{DynAny, StaticType};

use glam::{DAffine2, DVec2};
use std::collections::HashMap;

macro_rules! create_ids {
	($($id:ident),*) => {
		$(
			#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, DynAny)]
			#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
			/// A strongly typed ID
			pub struct $id(u64);

			impl $id {
				pub const ZERO: $id = $id(0);

				/// Generate a new random id
				pub fn generate() -> Self {
					Self(crate::uuid::generate_uuid())
				}

				pub fn inner(self) -> u64 {
					self.0
				}

				pub fn next_id(self) -> Self {
					Self(self.0 + 1)
				}
			}
		)*
	};
}

create_ids! { PointId, SegmentId, RegionId, StrokeId, FillId }

#[derive(Clone, Debug, Default, PartialEq, DynAny)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Stores data which is per-point. Each point is merely a position and can be used in a point cloud or to for a bézier path. In future this will be extendable at runtime with custom attributes.
pub struct PointDomain {
	id: Vec<PointId>,
	positions: Vec<DVec2>,
	g1_continous: Vec<Vec<[SegmentId; 2]>>,
}

impl core::hash::Hash for PointDomain {
	fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
		self.id.hash(state);
		self.positions.iter().for_each(|pos| pos.to_array().map(|v| v.to_bits()).hash(state));
	}
}

impl PointDomain {
	pub const fn new() -> Self {
		Self {
			id: Vec::new(),
			positions: Vec::new(),
			g1_continous: Vec::new(),
		}
	}

	pub fn clear(&mut self) {
		self.id.clear();
		self.positions.clear();
		self.g1_continous.clear();
	}

	pub fn retain(&mut self, f: impl Fn(&PointId) -> bool) {
		let mut keep = self.id.iter().map(&f);
		self.positions.retain(|_| keep.next().unwrap_or_default());
		let mut keep = self.id.iter().map(&f);
		self.g1_continous.retain(|_| keep.next().unwrap_or_default());
		self.id.retain(f);
	}

	pub fn push(&mut self, id: PointId, position: DVec2, g1_continous: Vec<[SegmentId; 2]>) {
		if self.id.contains(&id) {
			warn!("Duplicate point");
			return;
		}
		self.id.push(id);
		self.positions.push(position);
		self.g1_continous.push(g1_continous);
	}

	pub fn positions(&self) -> &[DVec2] {
		&self.positions
	}

	pub fn positions_mut(&mut self) -> impl Iterator<Item = (PointId, &mut DVec2)> {
		self.id.iter().copied().zip(self.positions.iter_mut())
	}
	pub fn g1_continous_mut(&mut self) -> impl Iterator<Item = (PointId, &mut Vec<[SegmentId; 2]>)> {
		self.id.iter().copied().zip(self.g1_continous.iter_mut())
	}

	pub fn ids(&self) -> &[PointId] {
		&self.id
	}

	pub fn pos_from_id(&self, id: PointId) -> Option<DVec2> {
		let pos = self.resolve_id(id).map(|index| self.positions[index]);
		if pos.is_none() {
			warn!("Resolving pos of invalid id");
		}
		pos
	}

	fn resolve_id(&self, id: PointId) -> Option<usize> {
		self.id.iter().position(|&check_id| check_id == id)
	}

	fn concat(&mut self, other: &Self, transform: DAffine2, id_map: &IdMap) {
		self.id.extend(other.id.iter().map(|id| *id_map.point_map.get(id).unwrap_or(id)));
		self.positions.extend(other.positions.iter().map(|&pos| transform.transform_point2(pos)));
	}

	fn transform(&mut self, transform: DAffine2) {
		for pos in &mut self.positions {
			*pos = transform.transform_point2(*pos);
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Hash, DynAny)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Stores data which is per-segment. A segment is a bézier curve between two end points with a stroke. In future this will be extendable at runtime with custom attributes.
pub struct SegmentDomain {
	ids: Vec<SegmentId>,
	start_point: Vec<PointId>,
	end_point: Vec<PointId>,
	handles: Vec<bezier_rs::BezierHandles>,
	stroke: Vec<StrokeId>,
}

impl SegmentDomain {
	pub const fn new() -> Self {
		Self {
			ids: Vec::new(),
			start_point: Vec::new(),
			end_point: Vec::new(),
			handles: Vec::new(),
			stroke: Vec::new(),
		}
	}

	pub fn clear(&mut self) {
		self.ids.clear();
		self.start_point.clear();
		self.end_point.clear();
		self.handles.clear();
		self.stroke.clear();
	}

	pub fn retain(&mut self, f: impl Fn(&SegmentId) -> bool) {
		let mut keep = self.ids.iter().map(&f);
		self.start_point.retain(|_| keep.next().unwrap_or_default());
		let mut keep = self.ids.iter().map(&f);
		self.end_point.retain(|_| keep.next().unwrap_or_default());
		let mut keep = self.ids.iter().map(&f);
		self.handles.retain(|_| keep.next().unwrap_or_default());
		let mut keep = self.ids.iter().map(&f);
		self.stroke.retain(|_| keep.next().unwrap_or_default());
		self.ids.retain(f);
	}

	pub fn push(&mut self, id: SegmentId, start: PointId, end: PointId, handles: bezier_rs::BezierHandles, stroke: StrokeId) {
		if self.ids.contains(&id) {
			warn!("Duplicate segment");
			return;
		}
		// Attempt to keep line joins?
		let after = self.end_point.iter().copied().position(|other_end| other_end == start || other_end == end);
		let before = self.start_point.iter().copied().position(|other_start| other_start == start || other_start == end);
		let (index, flip) = match (before, after) {
			(Some(before), Some(after)) if before < after => (before, self.start_point[before] == start),
			(Some(before), None) => (before, self.start_point[before] == start),

			(_, Some(after)) => (after + 1, self.end_point[after] == end),

			(None, None) => (self.ids.len(), false),
		};
		self.ids.insert(index, id);
		self.start_point.insert(index, start);
		self.end_point.insert(index, end);
		self.handles.insert(index, if flip { handles.flipped() } else { handles });
		self.stroke.insert(index, stroke);
	}

	pub fn start_point_mut(&mut self) -> impl Iterator<Item = (SegmentId, &mut PointId)> {
		self.ids.iter().copied().zip(self.start_point.iter_mut())
	}
	pub fn end_point_mut(&mut self) -> impl Iterator<Item = (SegmentId, &mut PointId)> {
		self.ids.iter().copied().zip(self.end_point.iter_mut())
	}
	pub fn handles_mut(&mut self) -> impl Iterator<Item = (SegmentId, &mut bezier_rs::BezierHandles)> {
		self.ids.iter().copied().zip(self.handles.iter_mut())
	}
	pub fn stroke_mut(&mut self) -> impl Iterator<Item = (SegmentId, &mut StrokeId)> {
		self.ids.iter().copied().zip(self.stroke.iter_mut())
	}

	fn resolve_id(&self, id: SegmentId) -> Option<usize> {
		self.ids.iter().position(|&check_id| check_id == id)
	}

	fn resolve_range(&self, range: &core::ops::RangeInclusive<SegmentId>) -> Option<core::ops::RangeInclusive<usize>> {
		match (self.resolve_id(*range.start()), self.resolve_id(*range.end())) {
			(Some(start), Some(end)) => Some(start..=end),
			_ => {
				warn!("Resolving range with invalid id");
				None
			}
		}
	}

	fn concat(&mut self, other: &Self, transform: DAffine2, id_map: &IdMap) {
		self.ids.extend(other.ids.iter().map(|id| *id_map.segment_map.get(id).unwrap_or(id)));
		self.start_point.extend(other.start_point.iter().map(|id| *id_map.point_map.get(id).unwrap_or(id)));
		self.end_point.extend(other.end_point.iter().map(|id| *id_map.point_map.get(id).unwrap_or(id)));
		self.handles.extend(other.handles.iter().map(|handles| handles.apply_transformation(|p| transform.transform_point2(p))));
		self.stroke.extend(&other.stroke);
	}

	fn transform(&mut self, transform: DAffine2) {
		for handles in &mut self.handles {
			*handles = handles.apply_transformation(|p| transform.transform_point2(p));
		}
	}

	pub fn connected(&self, point: PointId) -> usize {
		self.start_point.iter().filter(|&&val| val == point).count() + self.end_point.iter().filter(|&&val| val == point).count()
	}
}

#[derive(Clone, Debug, Default, PartialEq, Hash, DynAny)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Stores data which is per-region. A region is an enclosed area composed of a range of segments from the [`SegmentDomain`] that can be given a fill. In future this will be extendable at runtime with custom attributes.
pub struct RegionDomain {
	ids: Vec<RegionId>,
	segment_range: Vec<core::ops::RangeInclusive<SegmentId>>,
	fill: Vec<FillId>,
}

impl RegionDomain {
	pub const fn new() -> Self {
		Self {
			ids: Vec::new(),
			segment_range: Vec::new(),
			fill: Vec::new(),
		}
	}

	pub fn clear(&mut self) {
		self.ids.clear();
		self.segment_range.clear();
		self.fill.clear();
	}

	pub fn retain(&mut self, f: impl Fn(&RegionId) -> bool) {
		let mut keep = self.ids.iter().map(&f);
		self.segment_range.retain(|_| keep.next().unwrap_or_default());
		let mut keep = self.ids.iter().map(&f);
		self.fill.retain(|_| keep.next().unwrap_or_default());
		self.ids.retain(&f);
	}

	pub fn push(&mut self, id: RegionId, segment_range: core::ops::RangeInclusive<SegmentId>, fill: FillId) {
		if self.ids.contains(&id) {
			warn!("Duplicate region");
			return;
		}
		self.ids.push(id);
		self.segment_range.push(segment_range);
		self.fill.push(fill);
	}

	fn _resolve_id(&self, id: RegionId) -> Option<usize> {
		self.ids.iter().position(|&check_id| check_id == id)
	}

	pub fn segment_range_mut(&mut self) -> impl Iterator<Item = (RegionId, &mut core::ops::RangeInclusive<SegmentId>)> {
		self.ids.iter().copied().zip(self.segment_range.iter_mut())
	}

	pub fn fill_mut(&mut self) -> impl Iterator<Item = (RegionId, &mut FillId)> {
		self.ids.iter().copied().zip(self.fill.iter_mut())
	}

	fn concat(&mut self, other: &Self, _transform: DAffine2, id_map: &IdMap) {
		self.ids.extend(other.ids.iter().map(|id| *id_map.region_map.get(id).unwrap_or(id)));
		self.segment_range.extend(
			other
				.segment_range
				.iter()
				.map(|range| *id_map.segment_map.get(range.start()).unwrap_or(range.start())..=*id_map.segment_map.get(range.end()).unwrap_or(range.end())),
		);
		self.fill.extend(&other.fill);
	}
}

impl super::VectorData {
	/// Construct a [`bezier_rs::Bezier`] curve spanning from the resolved position of the start and end points with the specified handles. Returns [`None`] if either ID is invalid.
	fn segment_to_bezier(&self, start: PointId, end: PointId, handles: bezier_rs::BezierHandles) -> Option<bezier_rs::Bezier> {
		let start = self.point_domain.pos_from_id(start)?;
		let end = self.point_domain.pos_from_id(end)?;
		Some(bezier_rs::Bezier { start, end, handles })
	}

	/// Tries to convert a segment with the specified id to a [`bezier_rs::Bezier`], returning None if the id is invalid.
	pub fn segment_from_id(&self, id: SegmentId) -> Option<bezier_rs::Bezier> {
		let index = self.segment_domain.resolve_id(id)?;
		self.segment_to_bezier(self.segment_domain.start_point[index], self.segment_domain.end_point[index], self.segment_domain.handles[index])
	}

	/// Iterator over all of the [`bezier_rs::Bezier`] following the order that they are stored in the segment domain, skipping invalid segments.
	pub fn segment_bezier_iter(&self) -> impl Iterator<Item = (SegmentId, bezier_rs::Bezier, PointId, PointId)> + '_ {
		let to_bezier = |(((&handles, &id), &start), &end)| self.segment_to_bezier(start, end, handles).map(|bezier| (id, bezier, start, end));
		self.segment_domain
			.handles
			.iter()
			.zip(&self.segment_domain.ids)
			.zip(&self.segment_domain.start_point)
			.zip(&self.segment_domain.end_point)
			.filter_map(to_bezier)
	}

	/// Construct a [`bezier_rs::Bezier`] curve from an iterator of segments with (handles, start point, end point). Returns None if any ids are invalid or if the semgents are not continuous.
	fn subpath_from_segments(&self, segments: impl Iterator<Item = (bezier_rs::BezierHandles, PointId, PointId)>) -> Option<bezier_rs::Subpath<PointId>> {
		let mut first_point = None;
		let mut groups = Vec::new();
		let mut last: Option<(PointId, bezier_rs::BezierHandles)> = None;

		for (handle, start, end) in segments {
			if last.is_some_and(|(previous_end, _)| previous_end != start) {
				warn!("subpath_from_segments that were not continuous");
				return None;
			}
			first_point = Some(first_point.unwrap_or(start));

			groups.push(bezier_rs::ManipulatorGroup {
				anchor: self.point_domain.pos_from_id(start)?,
				in_handle: last.and_then(|(_, handle)| handle.end()),
				out_handle: handle.start(),
				id: start,
			});

			last = Some((end, handle));
		}

		let closed = groups.len() > 1 && last.map(|(point, _)| point) == first_point;

		if let Some((end, last_handle)) = last {
			if closed {
				groups[0].in_handle = last_handle.end();
			} else {
				groups.push(bezier_rs::ManipulatorGroup {
					anchor: self.point_domain.pos_from_id(end)?,
					in_handle: last_handle.end(),
					out_handle: None,
					id: end,
				});
			}
		}
		Some(bezier_rs::Subpath::new(groups, closed))
	}

	/// Construct a [`bezier_rs::Bezier`] curve for each region, skipping invalid regions.
	pub fn region_bezier_paths(&self) -> impl Iterator<Item = (RegionId, bezier_rs::Subpath<PointId>)> + '_ {
		self.region_domain
			.ids
			.iter()
			.zip(&self.region_domain.segment_range)
			.filter_map(|(&id, segment_range)| self.segment_domain.resolve_range(segment_range).map(|range| (id, range)))
			.filter_map(|(id, range)| {
				let segments_iter = self.segment_domain.handles[range.clone()]
					.iter()
					.zip(&self.segment_domain.start_point[range.clone()])
					.zip(&self.segment_domain.end_point[range])
					.map(|((&handles, &start), &end)| (handles, start, end));

				self.subpath_from_segments(segments_iter).map(|subpath| (id, subpath))
			})
	}

	/// Construct a [`bezier_rs::Bezier`] curve for stroke.
	pub fn stroke_bezier_paths(&self) -> StrokePathIter<'_> {
		StrokePathIter { vector_data: self, segment_index: 0 }
	}

	/// Construct an iterator [`bezier_rs::ManipulatorGroup`] for stroke.
	pub fn manipulator_groups(&self) -> impl Iterator<Item = bezier_rs::ManipulatorGroup<PointId>> + '_ {
		self.stroke_bezier_paths().flat_map(|mut path| std::mem::take(path.manipulator_groups_mut()))
	}

	/// Get manipulator by id
	pub fn manipulator_group_id(&self, id: impl Into<PointId>) -> Option<bezier_rs::ManipulatorGroup<PointId>> {
		let id = id.into();
		self.manipulator_groups().find(|group| group.id == id)
	}

	/// Transforms this vector data
	pub fn transform(&mut self, transform: DAffine2) {
		self.point_domain.transform(transform);
		self.segment_domain.transform(transform);
	}
}

#[derive(Clone)]
pub struct StrokePathIter<'a> {
	vector_data: &'a super::VectorData,
	segment_index: usize,
}

impl<'a> Iterator for StrokePathIter<'a> {
	type Item = bezier_rs::Subpath<PointId>;

	fn next(&mut self) -> Option<Self::Item> {
		let segments = &self.vector_data.segment_domain;
		if self.segment_index >= segments.end_point.len() {
			return None;
		}
		let mut old_end = None;
		let mut count = 0;
		let segments_iter = segments.handles[self.segment_index..]
			.iter()
			.zip(&segments.start_point[self.segment_index..])
			.zip(&segments.end_point[self.segment_index..])
			.map(|((&handles, &start), &end)| (handles, start, end))
			.take_while(|&(_, start, end)| {
				let continuous = old_end.is_none() || old_end.is_some_and(|old_end| old_end == start);
				old_end = Some(end);
				count += 1;
				continuous
			});

		let subpath = self.vector_data.subpath_from_segments(segments_iter);
		self.segment_index += count;
		subpath
	}
}

impl bezier_rs::Identifier for PointId {
	fn new() -> Self {
		Self::generate()
	}
}

impl crate::vector::ConcatElement for super::VectorData {
	fn concat(&mut self, other: &Self, transform: glam::DAffine2) {
		let new_ids = other.point_domain.id.iter().filter(|id| self.point_domain.id.contains(id)).map(|&old| (old, PointId::generate()));
		let point_map = new_ids.collect::<HashMap<_, _>>();
		let new_ids = other
			.segment_domain
			.ids
			.iter()
			.filter(|id| self.segment_domain.ids.contains(id))
			.map(|&old| (old, SegmentId::generate()));
		let segment_map = new_ids.collect::<HashMap<_, _>>();
		let new_ids = other.region_domain.ids.iter().filter(|id| self.region_domain.ids.contains(id)).map(|&old| (old, RegionId::generate()));
		let region_map = new_ids.collect::<HashMap<_, _>>();
		let id_map = IdMap { point_map, segment_map, region_map };
		self.point_domain.concat(&other.point_domain, transform * other.transform, &id_map);
		self.segment_domain.concat(&other.segment_domain, transform * other.transform, &id_map);
		self.region_domain.concat(&other.region_domain, transform * other.transform, &id_map);
		// TODO: properly deal with fills such as gradients
		self.style = other.style.clone();
		self.colinear_manipulators.extend(other.colinear_manipulators.iter().copied());
		self.alpha_blending = other.alpha_blending;
	}
}

struct IdMap {
	point_map: HashMap<PointId, PointId>,
	segment_map: HashMap<SegmentId, SegmentId>,
	region_map: HashMap<RegionId, RegionId>,
}
