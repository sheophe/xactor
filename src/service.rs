use crate::actor::ActorManager;
use crate::error::Result;
use crate::{Actor, Addr};
use anyhow::anyhow;
use fnv::FnvHasher;
use futures::lock::Mutex;
use futures::Future;
use once_cell::sync::OnceCell;
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;

static REGISTRY: OnceCell<
    Mutex<HashMap<TypeId, Box<dyn Any + Send>, BuildHasherDefault<FnvHasher>>>,
> = OnceCell::new();

/// Trait define a global service.
///
/// The service is a global actor.
/// You can use `Actor::from_registry` to get the address `Addr<A>` of the service.
///
/// # Examples
///
/// ```rust
/// use xactor::*;
///
/// #[message(result = "i32")]
/// struct AddMsg(i32);
///
/// #[derive(Default)]
/// struct MyService(i32);
///
/// impl Actor for MyService {}
///
/// impl Service for MyService {}
///
/// #[async_trait::async_trait]
/// impl Handler<AddMsg> for MyService {
///     async fn handle(&mut self, ctx: &mut Context<Self>, msg: AddMsg) -> i32 {
///         self.0 += msg.0;
///         self.0
///     }
/// }
///
/// #[xactor::main]
/// async fn main() -> Result<()> {
///     let mut addr = MyService::from_registry().await?;
///     assert_eq!(addr.call(AddMsg(1)).await?, 1);
///     assert_eq!(addr.call(AddMsg(5)).await?, 6);
///     Ok(())
/// }
/// ```
pub trait Service: Actor {
    fn start_service(self) -> impl Future<Output = Result<Addr<Self>>> + Send {
        async move {
            let registry = REGISTRY.get_or_init(Default::default);
            let mut registry = registry.lock().await;
            let actor_manager = ActorManager::new();
            registry.insert(TypeId::of::<Self>(), Box::new(actor_manager.address()));
            drop(registry);

            actor_manager.start_actor(self).await
        }
    }

    fn from_registry() -> impl Future<Output = Result<Addr<Self>>> + Send {
        async move {
            let registry = REGISTRY.get().ok_or(anyhow!("registry not initialized"))?;
            let mut registry = registry.lock().await;

            match registry.get_mut(&TypeId::of::<Self>()) {
                Some(addr) => Ok(addr.downcast_ref::<Addr<Self>>().unwrap().clone()),
                None => Err(anyhow!("service not found")),
            }
        }
    }
}

thread_local! {
    static LOCAL_REGISTRY: RefCell<HashMap<TypeId, Box<dyn Any + Send>, BuildHasherDefault<FnvHasher>>> = Default::default();
}

/// Trait define a local service.
///
/// The service is a thread local actor.
/// You can use `Actor::from_registry` to get the address `Addr<A>` of the service.
pub trait LocalService: Actor {
    fn start_service(self) -> impl Future<Output = Result<Addr<Self>>> + Send {
        async move {
            let res = LOCAL_REGISTRY.with(|registry| {
                registry
                    .borrow_mut()
                    .get_mut(&TypeId::of::<Self>())
                    .map(|addr| addr.downcast_ref::<Addr<Self>>().unwrap().clone())
            });
            match res {
                Some(addr) => Ok(addr),
                None => {
                    let addr = ActorManager::new().start_actor(self).await?;
                    LOCAL_REGISTRY.with(|registry| {
                        registry
                            .borrow_mut()
                            .insert(TypeId::of::<Self>(), Box::new(addr.clone()));
                    });
                    Ok(addr)
                }
            }
        }
    }

    fn from_registry() -> impl Future<Output = Result<Addr<Self>>> + Send {
        async move {
            let res = LOCAL_REGISTRY.with(|registry| {
                registry
                    .borrow_mut()
                    .get_mut(&TypeId::of::<Self>())
                    .map(|addr| addr.downcast_ref::<Addr<Self>>().unwrap().clone())
            });
            match res {
                Some(addr) => Ok(addr),
                None => Err(anyhow!("service not found")),
            }
        }
    }
}
