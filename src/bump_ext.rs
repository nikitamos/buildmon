use core::slice;

use bumpalo::Bump;

pub(super) trait BumpEnlargeExt {
     fn enlarge_last_allocation<'a, T>(&'a self, alloc: &'a mut &'a [T], extend_by: usize);
}

impl BumpEnlargeExt for Bump {
    fn enlarge_last_allocation<'a, T>(&'a self, alloc: &'a mut &'a [T], extend_by: usize) {
        let extension: &mut [T] = self.alloc_slice_fill_with(extend_by, |_| todo!());
        if alloc.as_ptr_range().end == extension.as_ptr_range().start {
            *alloc = unsafe {slice::from_raw_parts(alloc.as_ptr(), alloc.len() + extension.len())};
        } else {
            panic!("Another allocation performed");
        }
    }
}
