use voidui::core::geometry::Spacing;

#[test]
fn integer_spacing_clamps_each_edge_without_overflow() {
    macro_rules! check {
        ($($number:ty),*) => {
            $(
                // Exercise both overflow directions for every public integer type.
                let min = Spacing::<$number>::uniform(<$number>::MIN);
                let max = Spacing::<$number>::uniform(<$number>::MAX);
                let zero = Spacing::<$number>::uniform(0);
                let one = Spacing::<$number>::uniform(1);

                assert_eq!(min.shrink(max), zero);
                assert_eq!(max.shrink(min), max);
                assert_eq!(max.shrink(max), zero);
                assert_eq!(max.expand(one), max);
                assert_eq!(min.expand(min), min);
                assert_eq!(
                    Spacing::<$number>::new(5, 0, 9, 4)
                        .shrink(Spacing::<$number>::new(9, 1, 2, 4)),
                    Spacing::<$number>::new(0, 0, 7, 0),
                );
                assert_eq!(
                    Spacing::<$number>::new(1, 2, 3, 4)
                        .expand(Spacing::<$number>::new(4, 3, 2, 1)),
                    Spacing::<$number>::uniform(5),
                );
            )*
        };
    }

    check!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
}

#[test]
fn signed_spacing_handles_negative_operands() {
    assert_eq!(
        Spacing::<i32>::new(-5, 5, -5, 5).shrink(Spacing::<i32>::new(-9, -9, 9, 9)),
        Spacing::<i32>::new(4, 14, 0, 0),
    );
}

#[test]
fn floating_point_spacing_preserves_fractional_arithmetic() {
    macro_rules! check {
        ($($number:ty),*) => {
            $(
                let spacing = Spacing::<$number>::new(1.5, 0.5, 2.5, 4.5);
                let delta = Spacing::<$number>::new(0.5, 1.5, 0.5, 4.5);
                assert_eq!(spacing.shrink(delta), Spacing::new(1.0, 0.0, 2.0, 0.0));
                assert_eq!(spacing.expand(delta), Spacing::new(2.0, 2.0, 3.0, 9.0));
            )*
        };
    }

    check!(f32, f64);
}

#[test]
fn spacing_supports_the_default_trait() {
    // Generic callers and derived defaults must use the same zero spacing.
    fn default_value<T: Default>() -> T {
        T::default()
    }

    assert_eq!(default_value::<Spacing<u32>>(), Spacing::uniform(0));
    assert_eq!(Spacing::<f32>::default(), Spacing::uniform(0.0));
}
