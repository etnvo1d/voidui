//! Prefix-height index. Updating one measured block or locating a scroll offset
//! is logarithmic; unmeasured blocks retain configurable estimates.
#[derive(Clone, Default)]
pub(crate) struct HeightIndex {
    values: Vec<f32>,
    tree: Vec<f64>,
    // Height knowledge outlives the bounded cache of shaped paragraphs.
    measured: Vec<bool>,
}
impl HeightIndex {
    pub fn new(values: Vec<f32>) -> Self {
        let mut tree = Vec::with_capacity(values.len() + 1);
        tree.push(0.0);
        tree.extend(values.iter().map(|v| f64::from(*v)));
        // Linear construction; online height corrections use logarithmic updates.
        for i in 1..tree.len() {
            let parent = i + (i & i.wrapping_neg());
            if parent < tree.len() {
                tree[parent] += tree[i];
            }
        }
        let measured = vec![false; values.len()];
        Self {
            values,
            tree,
            measured,
        }
    }
    pub fn set(&mut self, i: usize, value: f32) {
        let delta = f64::from(value) - f64::from(self.values[i]);
        self.measured[i] = true;
        self.values[i] = value;
        let mut n = i + 1;
        while n < self.tree.len() {
            self.tree[n] += delta;
            n += n & n.wrapping_neg();
        }
    }
    pub fn measurements(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
        self.values
            .iter()
            .zip(&self.measured)
            .enumerate()
            .filter_map(|(i, (&height, &measured))| measured.then_some((i, height)))
    }
    pub fn forget_measurements(&mut self) {
        // Old heights still locate the displayed anchor, but a changed view
        // registry must not reuse them as verified measurements.
        self.measured.fill(false);
    }
    pub fn top(&self, i: usize) -> f32 {
        let mut n = i.min(self.values.len());
        let mut sum = 0.0;
        while n > 0 {
            sum += self.tree[n];
            n &= n - 1;
        }
        sum as f32
    }
    pub fn total(&self) -> f32 {
        self.top(self.values.len())
    }
    pub fn at(&self, y: f32) -> usize {
        if self.values.is_empty() {
            return 0;
        }
        let mut index = 0;
        let mut sum = 0.0;
        let mut bit = self.values.len().next_power_of_two();
        while bit > 0 {
            let next = index + bit;
            if next < self.tree.len() && sum + self.tree[next] <= f64::from(y.max(0.0)) {
                index = next;
                sum += self.tree[next];
            }
            bit >>= 1;
        }
        index.min(self.values.len() - 1)
    }
}
