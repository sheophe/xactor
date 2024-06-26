use crate::addr::ActorEvent;
use crate::error::Result;
use crate::runtime::spawn;
use crate::{Addr, Context};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use futures::{Future, FutureExt, StreamExt};

/// Represents a message that can be handled by the actor.
pub trait Message: 'static + Send {
    /// The return value type of the message
    /// This type can be set to () if the message does not return a value, or if it is a notification message
    type Result: 'static + Send;
}

/// Describes how to handle messages of a specific type.
/// Implementing Handler is a general way to handle incoming messages.
/// The type T is a message which can be handled by the actor.
pub trait Handler<T: Message>: Actor
where
    Self: Sized,
{
    /// Method is called for every message received by this Actor.
    fn handle(&mut self, ctx: &mut Context<Self>, msg: T)
        -> impl Future<Output = T::Result> + Send;
}

/// Describes how to handle messages of a specific type.
/// Implementing Handler is a general way to handle incoming streams.
/// The type T is a stream message which can be handled by the actor.
/// Stream messages do not need to implement the `Message` trait.
#[allow(unused_variables)]
pub trait StreamHandler<T: 'static>: Actor {
    /// Method is called for every message received by this Actor.
    fn handle(&mut self, ctx: &mut Context<Self>, msg: T) -> impl Future<Output = ()> + Send;

    /// Method is called when stream get polled first time.
    fn started(&mut self, ctx: &mut Context<Self>) -> impl Future<Output = ()> + Send {
        async move {}
    }

    /// Method is called when stream finishes.
    ///
    /// By default this method stops actor execution.
    fn finished(&mut self, ctx: &mut Context<Self>) -> impl Future<Output = ()> + Send {
        async move {
            ctx.stop(None);
        }
    }
}

/// Actors are objects which encapsulate state and behavior.
/// Actors run within a specific execution context `Context<A>`.
/// The context object is available only during execution.
/// Each actor has a separate execution context.
///
/// Roles communicate by exchanging messages.
/// The requester can wait for a response.
/// By `Addr` referring to the actors, the actors must provide an `Handle<T>` implementation for this message.
/// All messages are statically typed.
#[allow(unused_variables)]
pub trait Actor: Sized + Send + 'static {
    /// Called when the actor is first started.
    fn started(&mut self, ctx: &mut Context<Self>) -> impl Future<Output = Result<()>> + Send {
        async move { Ok(()) }
    }

    /// Called after an actor is stopped.
    fn stopped(&mut self, ctx: &mut Context<Self>) -> impl Future<Output = ()> + Send {
        async move {}
    }

    /// Construct and start a new actor, returning its address.
    ///
    /// This is constructs a new actor using the `Default` trait, and invokes its `start` method.
    fn start_default() -> impl Future<Output = Result<Addr<Self>>> + Send
    where
        Self: Send + Default,
    {
        Self::default().start()
    }

    /// Start a new actor, returning its address.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use xactor::*;
    ///
    /// struct MyActor;
    ///
    /// impl Actor for MyActor {}
    ///
    /// #[message(result = "i32")]
    /// struct MyMsg(i32);
    ///
    /// #[async_trait::async_trait]
    /// impl Handler<MyMsg> for MyActor {
    ///     async fn handle(&mut self, _ctx: &mut Context<Self>, msg: MyMsg) -> i32 {
    ///         msg.0 * msg.0
    ///     }
    /// }
    ///
    /// #[xactor::main]
    /// async fn main() -> Result<()> {
    ///     // Start actor and get its address
    ///     let mut addr = MyActor.start().await?;
    ///
    ///     // Send message `MyMsg` to actor via addr
    ///     let res = addr.call(MyMsg(10)).await?;
    ///     assert_eq!(res, 100);
    ///     Ok(())
    /// }
    /// ```
    fn start(self) -> impl Future<Output = Result<Addr<Self>>> + Send {
        ActorManager::new().start_actor(self)
    }
}

pub(crate) struct ActorManager<A: Actor> {
    ctx: Context<A>,
    tx: std::sync::Arc<UnboundedSender<ActorEvent<A>>>,
    rx: UnboundedReceiver<ActorEvent<A>>,
    tx_exit: oneshot::Sender<()>,
}

impl<A: Actor> ActorManager<A> {
    pub(crate) fn new() -> Self {
        let (tx_exit, rx_exit) = oneshot::channel();
        let rx_exit = rx_exit.shared();
        let (ctx, rx, tx) = Context::new(Some(rx_exit));
        Self {
            ctx,
            rx,
            tx,
            tx_exit,
        }
    }

    pub(crate) fn address(&self) -> Addr<A> {
        self.ctx.address()
    }

    pub(crate) async fn start_actor(self, mut actor: A) -> Result<Addr<A>> {
        let Self {
            mut ctx,
            mut rx,
            tx,
            tx_exit,
        } = self;

        let rx_exit = ctx.rx_exit.clone();
        let actor_id = ctx.actor_id();

        // Call started
        actor.started(&mut ctx).await?;

        spawn({
            async move {
                while let Some(event) = rx.next().await {
                    match event {
                        ActorEvent::Exec(f) => f(&mut actor, &mut ctx).await,
                        ActorEvent::Stop(_err) => break,
                        ActorEvent::RemoveStream(id) => {
                            let mut streams = ctx.streams.lock().unwrap();

                            if streams.contains(id) {
                                streams.remove(id);
                            }
                        }
                    }
                }

                actor.stopped(&mut ctx).await;

                ctx.abort_streams();
                ctx.abort_intervals();

                tx_exit.send(()).ok();
            }
        });

        Ok(Addr {
            actor_id,
            tx,
            rx_exit,
        })
    }
}
