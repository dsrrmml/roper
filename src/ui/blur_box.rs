use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::Cell;

const DEFAULT_BLUR_RADIUS: f64 = 8.0;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct BlurBox {
        pub blurred: Cell<bool>,
        pub radius: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BlurBox {
        const NAME: &'static str = "RoperBlurBox";
        type Type = super::BlurBox;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for BlurBox {}

    impl WidgetImpl for BlurBox {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if self.blurred.get() {
                snapshot.push_blur(self.radius.get());
                self.parent_snapshot(snapshot);
                snapshot.pop();
            } else {
                self.parent_snapshot(snapshot);
            }
        }
    }

    impl BoxImpl for BlurBox {}
}

glib::wrapper! {
    pub struct BlurBox(ObjectSubclass<imp::BlurBox>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl BlurBox {
    pub fn new() -> Self {
        let box_: Self = glib::Object::builder()
            .property("orientation", gtk::Orientation::Vertical)
            .build();
        box_.imp().radius.set(DEFAULT_BLUR_RADIUS);
        box_
    }

    pub fn set_blurred(&self, blurred: bool) {
        if self.imp().blurred.replace(blurred) != blurred {
            self.queue_draw();
        }
    }
}

impl Default for BlurBox {
    fn default() -> Self {
        Self::new()
    }
}
