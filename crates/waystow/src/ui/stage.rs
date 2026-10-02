//! A container with a fixed size that places each child at a rectangle with
//! an extra transform. Unlike `gtk::Fixed`, transforms never change its size,
//! so animating a child doesn't resize the layer surface every frame.

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{graphene, gsk};

/// A child, its rectangle and its extra transform.
type Slot = (gtk::Widget, (i32, i32, i32, i32), Option<gsk::Transform>);

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Stage {
        pub size: RefCell<(i32, i32)>,
        pub slots: RefCell<Vec<Slot>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Stage {
        const NAME: &'static str = "WaystowStage";
        type Type = super::Stage;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Stage {
        fn dispose(&self) {
            for (w, _, _) in self.slots.borrow_mut().drain(..) {
                w.unparent();
            }
        }
    }

    impl WidgetImpl for Stage {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            // Children are measured too (GTK requires it before allocation).
            for (w, _, _) in self.slots.borrow().iter() {
                let _ = w.measure(orientation, -1);
            }
            let (w, h) = *self.size.borrow();
            let v = if orientation == gtk::Orientation::Horizontal { w } else { h };
            (v, v, -1, -1)
        }

        fn size_allocate(&self, _: i32, _: i32, _: i32) {
            for (w, (x, y, cw, ch), extra) in self.slots.borrow().iter() {
                if !w.is_visible() {
                    continue;
                }
                let base = gsk::Transform::new().translate(&graphene::Point::new(*x as f32, *y as f32));
                let t = match extra {
                    Some(e) => base.transform(Some(e)),
                    None => base,
                };
                // Never below the child's minimum (GTK warns and clips otherwise).
                let mw = w.measure(gtk::Orientation::Horizontal, -1).0;
                let mh = w.measure(gtk::Orientation::Vertical, (*cw).max(mw)).0;
                w.allocate((*cw).max(mw), (*ch).max(mh), -1, Some(t));
            }
        }
    }
}

glib::wrapper! {
    pub struct Stage(ObjectSubclass<imp::Stage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Stage {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Stage {
    /// Set the stage size.
    pub fn set_size(&self, w: i32, h: i32) {
        *self.imp().size.borrow_mut() = (w, h);
        self.queue_resize();
    }

    /// Add a child.
    pub fn add(&self, child: &impl IsA<gtk::Widget>) {
        child.set_parent(self);
        self.imp().slots.borrow_mut().push((child.clone().upcast(), (0, 0, 0, 0), None));
    }

    /// Place a child at a rectangle.
    pub fn place(&self, child: &impl IsA<gtk::Widget>, x: i32, y: i32, w: i32, h: i32) {
        if let Some(s) = self.imp().slots.borrow_mut().iter_mut().find(|s| &s.0 == child.as_ref()) {
            s.1 = (x, y, w, h);
        }
        self.queue_allocate();
    }

    /// Set a child's extra transform (relative to its rectangle's origin).
    pub fn set_transform(&self, child: &impl IsA<gtk::Widget>, t: Option<gsk::Transform>) {
        if let Some(s) = self.imp().slots.borrow_mut().iter_mut().find(|s| &s.0 == child.as_ref()) {
            s.2 = t;
        }
        self.queue_allocate();
    }
}
