use std::cell::RefCell;
use std::rc::{Rc, Weak};

#[derive(Debug)]
struct NodeStrong {
    next: RefCell<Option<Rc<NodeStrong>>>,
}

#[test]
fn strong_cycle_keeps_objects_alive() {
    // Two nodes that hold strong Rc to each other -> cycle
    let a = Rc::new(NodeStrong { next: RefCell::new(None) });
    let b = Rc::new(NodeStrong { next: RefCell::new(None) });

    *a.next.borrow_mut() = Some(Rc::clone(&b));
    *b.next.borrow_mut() = Some(Rc::clone(&a));

    let a_weak = Rc::downgrade(&a);
    let b_weak = Rc::downgrade(&b);

    // drop original strong handles
    drop(a);
    drop(b);

    // Because of the strong cycle, upgrade still succeeds -> objects remain (leak)
    assert!(a_weak.upgrade().is_some(), "a should still be alive due to strong cycle");
    assert!(b_weak.upgrade().is_some(), "b should still be alive due to strong cycle");
}

#[derive(Debug)]
struct NodeWeak {
    next: RefCell<Option<Weak<NodeWeak>>>,
}

#[test]
fn weak_references_allow_drop() {
    // Two nodes that hold Weak refs to each other -> no cycle keeping them alive
    let a = Rc::new(NodeWeak { next: RefCell::new(None) });
    let b = Rc::new(NodeWeak { next: RefCell::new(None) });

    *a.next.borrow_mut() = Some(Rc::downgrade(&b));
    *b.next.borrow_mut() = Some(Rc::downgrade(&a));

    let a_weak = Rc::downgrade(&a);
    let b_weak = Rc::downgrade(&b);

    // drop original strong handles
    drop(a);
    drop(b);

    // Now upgrade should fail because only weak links remain
    assert!(a_weak.upgrade().is_none(), "a should be dropped when only weak links remain");
    assert!(b_weak.upgrade().is_none(), "b should be dropped when only weak links remain");
}
