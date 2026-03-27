pub trait ContainsByteSlice {
    fn contains_slice(&self, slice: &[u8]) -> bool;
}

impl ContainsByteSlice for Vec<u8> {
    fn contains_slice(&self, slice: &[u8]) -> bool {
        if slice.len() == 0 {
            return true;
        }
        self.windows(slice.len()).any(|window| window == slice)
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_contains_slice() {
        use super::ContainsByteSlice;
        // Test data
        let vec = vec![1, 2, 3, 4, 5, 6, 7, 8];

        // Slice is at the beginning
        assert!(vec.contains_slice(&[1, 2, 3][..]));

        // Slice is in the middle
        assert!(vec.contains_slice(&[3, 4, 5][..]));

        // Slice is at the end
        assert!(vec.contains_slice(&[6, 7, 8][..]));

        // Single element slice
        assert!(vec.contains_slice(&[5][..]));

        // Slice not present
        assert!(!vec.contains_slice(&[9, 10][..]));

        // Empty slice (should always return true)
        assert!(vec.contains_slice(&[][..]));

        // Slice longer than vec (should return false)
        assert!(!vec.contains_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9][..]));

        let empty = vec![];
        // Empty vec (should return false unless slice is also empty)
        assert!(!empty.contains_slice(&[1][..]));
        assert!(empty.contains_slice(&[][..]));
    }

    #[test]
    fn test_contains_slice_edge_cases() {
        use super::ContainsByteSlice;
        // All elements match
        assert!(vec![1, 1, 1].contains_slice(&[1, 1][..]));

        // Repeated elements
        assert!(vec![1, 2, 1, 2, 1].contains_slice(&[1, 2, 1][..]));

        // Overlapping matches
        assert!(vec![1, 2, 2, 1].contains_slice(&[2, 2][..]));
    }
}
