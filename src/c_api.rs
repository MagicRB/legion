use crate::storage::ComponentTypeId;
use std::ffi::c_void;
use std::any::TypeId;
use std::cell::RefMut;
use std::ops::Deref;

#[repr(C)]
pub struct Universe {
    _private: [u8; 0],
}
#[repr(C)]
pub struct World {
    _private: [u8; 0],
}

impl From<*mut World> for &mut crate::prelude::World {
    fn from(world: *mut World) -> Self {
        unsafe { std::mem::transmute::<&mut World, &mut crate::prelude::World>(world.as_mut().unwrap()) }
    }
}

impl From<&mut crate::prelude::World> for *mut World {
    fn from(world: &mut crate::prelude::World) -> Self {
        unsafe { std::mem::transmute::<&mut crate::prelude::World, &mut World>(world) }
    }
}

#[repr(C)]
pub struct Entity {
    index: u32,
    version: u32
}

// @TODO not the best, since it could theoretically be laid-out differently

impl From<crate::prelude::Entity> for Entity {
    fn from(entity: crate::prelude::Entity) -> Self {
        unsafe { std::mem::transmute::<crate::prelude::Entity, Entity>(entity) }
    }
}

impl From<Entity> for crate::prelude::Entity {
    fn from(entity: Entity) -> Self {
        unsafe { std::mem::transmute::<Entity, crate::prelude::Entity>(entity) }
    }
}

pub struct CApiComponent {
    name: &'static str,
    typeid: u64
}

impl CApiComponent {
    pub fn new(name: &'static str, typeid: u64) -> Self {
        CApiComponent { name, typeid }
    }
}

inventory::collect!(CApiComponent);

//pub struct ExternalComponent {}

//#[repr(C)]
//pub struct EntityData {
//    // The number of tag types in the entity's archetype
//    pub num_tag_types: u32,
//    // An array of tag types in the entity's archetype. Length == num_tag_types
//    pub tag_types: *const u32,
//    // An array of the size of each tag type, indices corresponding to `tag_types`
//    pub tag_data_sizes: *const u32,
//    // Array of pointers to data for each tag. Length == num_tag_types
//    pub tag_data: *const *const c_void,
//    // The number of component types in the entity's archetype
//    pub num_component_types: u32,
//    // An array of component types in the entity's archetype. Length == num_component_types
//    pub component_types: *const u32,
//    // An array of the size of each component type, indices corresponding to `component_types`
//    pub component_data_sizes: *const u32,
//    // Number of entities to insert
//    pub num_entities: u32,
//    // An array of pointers to component data per type. Indices correspond to `component_types`.
//    // Each pointer in the array points to an array of component data with the type of the corresponding entry in  `component_types`, with length of the array being equal to `num_entities`.
//    pub component_data: *const *const c_void,
//    /// Optionally specify pre-allocated entityIDs.
//    /// Pass null if entity IDs should be allocated when inserting data.
//    /// Length must be equal to num_entities.
//    pub entity_ids: *const Entity,
//}

//fn lgn_world_get_component(ptr: *mut World, ty: u32, entity: Entity) -> *mut c_void {
//    let world = unsafe { (ptr as *mut crate::prelude::World).as_mut().expect("universe null ptr") }; // @TODO better error perhaps
//    let entity: crate::prelude::Entity = entity.into();
//
//    if !world.is_alive(entity) {
//        // @TODO return
//    }
//
//    let location = world.entity_allocator.get_location(entity.index()).unwrap();  // @TODO better error
//    let archetype = world.storage().archetypes().get(location.archetype()).unwrap(); // @TODO better error
//    let chunk = archetype
//        .chunksets()
//        .get(location.set()).unwrap()  // @TODO better error
//        .get(location.chunk()).unwrap();  // @TODO better error
//    let (slice, size, count) =
//        chunk
//            .components(ComponentTypeId::of_c_api::<ExternalComponent>(ty)).unwrap()
//            .data_raw();
//
//    let (slice_borrow, slice) = unsafe { slice.deconstruct() };
//
//    unsafe { slice.offset((size * location.component()) as isize) as *mut c_void }
//}

fn lgn_world_get_rust_component(ptr: *mut World, ty: u64, entity: Entity) -> *mut c_void {
    let world = unsafe { (ptr as *mut crate::prelude::World).as_mut().expect("universe null ptr") }; // @TODO better error perhaps
    let entity: crate::prelude::Entity = entity.into();

    if !world.is_alive(entity) {
        panic!("AA")
    }

    let location = world.entity_allocator.get_location(entity.index()).unwrap();  // @TODO better error
    let archetype = world.storage().archetypes().get(location.archetype()).unwrap(); // @TODO better error
    let chunk = archetype
        .chunksets()
        .get(location.set()).unwrap()  // @TODO better error
        .get(location.chunk()).unwrap();  // @TODO better error
    let (slice, size, count) =
        chunk
            .components(unsafe { std::mem::transmute::<(u64, u32), ComponentTypeId>((ty, 0)) }).unwrap()
            .data_raw();
    let (slice_borrow, slice) = unsafe { slice.deconstruct() };

    unsafe { slice.offset((size * location.component()) as isize) as *mut c_void }
}

fn lgn_universe_new() -> *mut Universe {
    let universe = Box::new(crate::prelude::Universe::new());
    Box::into_raw(universe) as *mut Universe
}

fn lgn_universe_free(ptr: *mut Universe) {
    unsafe {
        let _universe = Box::from_raw(ptr as *mut crate::prelude::Universe);
        // let universe be dropped
    }
}

fn lgn_universe_create_world(ptr: *mut Universe) -> *mut World {
    unsafe {
        let world = Box::new(
            (ptr as *mut crate::prelude::Universe)
                .as_mut()
                .expect("universe null ptr")
                .create_world(),
        );
        Box::into_raw(world) as *mut World
    }
}

fn lgn_world_free(ptr: *mut World) -> () {
    unsafe {
        let _world = Box::from_raw(ptr as *mut crate::prelude::World);
        // let world be dropped
    }
}

#[cfg(test)]
mod test {
    use crate::c_api::{lgn_world_get_rust_component, World };
    use crate::storage::ComponentTypeId;
    use std::os::raw::c_void;

    struct Pos(f32, f32, f32);
    struct Vel(f32, f32, f32);

    #[test]
    fn get_rust_component() {
        let universe = crate::prelude::Universe::new();
        let mut world = universe.create_world();

        let entity = world.insert(
            (),
            vec![
                (Pos(1., 2., 3.), Vel(1., 2., 3.)),
            ],
        )[0].clone();

        let pos_id = unsafe { std::mem::transmute::<std::any::TypeId, u64>(std::any::TypeId::of::<Pos>()) };
        assert_eq!(unsafe { std::mem::transmute::<(u64, u32), ComponentTypeId>((pos_id, 0))}, ComponentTypeId::of::<Pos>());


        let ffi_pos = lgn_world_get_rust_component((&mut world).into(), pos_id, entity.into());

        let pos = unsafe { std::mem::transmute::<*mut c_void, &mut Pos>(ffi_pos) };

        assert_eq!(pos.0, 1.);
        assert_eq!(pos.1, 2.);
        assert_eq!(pos.2, 3.);
    }
}