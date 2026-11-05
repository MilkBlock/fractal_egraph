use super::Columns;
#[derive(Default)]
pub(super) struct TableDirectory(Vec<Option<Columns>>);
impl TableDirectory {
    #[inline]
    pub fn get(&self, t: &usize) -> Option<&Columns> {
        self.0.get(*t)?.as_ref()
    }
    #[inline]
    pub fn get_mut(&mut self, t: &usize) -> Option<&mut Columns> {
        self.0.get_mut(*t)?.as_mut()
    }
    pub fn contains_key(&self, t: &usize) -> bool {
        self.get(t).is_some()
    }
    pub fn insert(&mut self, t: usize, v: Columns) {
        if self.0.len() <= t {
            self.0.resize_with(t + 1, || None);
        }
        self.0[t] = Some(v);
    }
    pub fn remove(&mut self, t: &usize) -> Option<Columns> {
        self.0.get_mut(*t)?.take()
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&usize, &mut Columns) -> bool) {
        for (t, s) in self.0.iter_mut().enumerate() {
            if s.as_mut().is_some_and(|v| !keep(&t, v)) {
                *s = None;
            }
        }
    }
    pub fn clear(&mut self) {
        self.0.clear()
    }
    pub fn values(&self) -> impl Iterator<Item = &Columns> {
        self.0.iter().flatten()
    }
}
impl std::ops::Index<&usize> for TableDirectory {
    type Output = Columns;
    fn index(&self, t: &usize) -> &Columns {
        self.get(t).unwrap()
    }
}
