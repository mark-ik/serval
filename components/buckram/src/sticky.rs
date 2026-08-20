//! Scroll-dependent sticky-position constraints.

/// One physical-axis sticky constraint. The normal and containing coordinates
/// are in layout space; the scrollport offset is the amount its content has
/// moved away from that space before paint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickyAxisInput {
    pub normal_start: f32,
    pub box_size: f32,
    pub scrollport_start: f32,
    pub scrollport_size: f32,
    pub scroll_offset: f32,
    pub containing_start: f32,
    pub containing_size: f32,
    pub start_inset: Option<f32>,
    pub end_inset: Option<f32>,
}

/// The physical translation from a sticky box's normal-flow position.
///
/// The scrollport threshold is expressed in layout space by adding its
/// content scroll offset. A paint consumer can then apply ordinary scroll
/// translation after this result without rebuilding normal-flow geometry.
pub fn solve_sticky_axis(input: StickyAxisInput) -> f32 {
    let containing_end = input.containing_start + input.containing_size;
    let containing_max = containing_end - input.box_size;
    let start_limit = input
        .start_inset
        .map_or(f32::NEG_INFINITY, |inset| {
            input.scrollport_start + input.scroll_offset + inset
        })
        .max(input.containing_start);
    let end_limit = input
        .end_inset
        .map_or(f32::INFINITY, |inset| {
            input.scrollport_start + input.scroll_offset + input.scrollport_size
                - inset
                - input.box_size
        })
        .min(containing_max);

    let used = match (input.start_inset, input.end_inset) {
        (None, None) => input.normal_start,
        (Some(_), None) => input.normal_start.max(start_limit).min(end_limit),
        (None, Some(_)) => input.normal_start.min(end_limit).max(start_limit),
        (Some(_), Some(_)) => {
            if start_limit <= end_limit {
                input.normal_start.clamp(start_limit, end_limit)
            } else {
                // An over-constrained sticky view rectangle has no span. The
                // start-side constraint wins in the same physical direction
                // as the ordinary positioned inset solver.
                start_limit
            }
        },
    };
    used - input.normal_start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> StickyAxisInput {
        StickyAxisInput {
            normal_start: 40.0,
            box_size: 20.0,
            scrollport_start: 0.0,
            scrollport_size: 100.0,
            scroll_offset: 0.0,
            containing_start: 0.0,
            containing_size: 180.0,
            start_inset: None,
            end_inset: None,
        }
    }

    #[test]
    fn start_inset_moves_only_after_the_normal_position_crosses_it() {
        let mut constraint = input();
        constraint.start_inset = Some(8.0);
        assert_eq!(solve_sticky_axis(constraint), 0.0);
        constraint.scroll_offset = 50.0;
        assert_eq!(solve_sticky_axis(constraint), 18.0);
    }

    #[test]
    fn end_inset_limits_the_opposite_physical_side() {
        let mut constraint = input();
        constraint.normal_start = 90.0;
        constraint.end_inset = Some(6.0);
        assert_eq!(solve_sticky_axis(constraint), -16.0);
    }

    #[test]
    fn containing_block_stops_a_start_sticky_box() {
        let mut constraint = input();
        constraint.start_inset = Some(0.0);
        constraint.scroll_offset = 200.0;
        assert_eq!(solve_sticky_axis(constraint), 120.0);
    }
}
