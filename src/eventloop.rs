// Copyright 2016 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file,
// You can obtain one at http://mozilla.org/MPL/2.0/.


use std::{mem, thread, ptr};
use std::any::Any;
use std::sync::{Arc, Mutex};
use std::io::{self, Error};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::os::unix::io::{AsRawFd, RawFd};

use epoll::*;
use simple_slab::Slab;
use threadpool::ThreadPool;

use types::Handler;

use ::Connection;
use ::EventHandler;
use ::Stream;


type InterestList = Arc<Mutex<Vec<Interest>>>;
type ConnectionSlab<T: Stream> = Arc<Mutex<Slab<Arc<Connection<T>>>>>;
type NewConnectionQueue<T: Stream> = Arc<Mutex<Vec<Arc<Connection<T>>>>>;


static mut epoll: *mut EpollInstance = 0 as *mut EpollInstance;
static mut io_pool: *mut ThreadPool = 0 as *mut ThreadPool;


pub fn begin<A, S, E>(addr: A,
                      io_threads: usize,
                      event_handler: Box<E>) where
    A: ToSocketAddrs + Send + 'static,
    S: Stream + 'static,
    E: EventHandler<S> + Send + Sync + 'static
{
    // Unwrap our event handler into something we can share across threads.
    // Internal synchronization of the object is the responsibility of the
    // caller.
    let handler = Handler {
        inner: Box::into_raw(event_handler)
    };

    // Thread safe pool of new users waiting to be added to event loop.
    let new_conn_pool = Arc::new(Mutex::new(Vec::<Arc<Connection<S>>>::new()));

    // Start up the new connection listener
    let listener_copy_queue = new_conn_pool.clone();
    let listener_copy_handler = handler.clone();
    thread::spawn(move || {
        listen(addr, listener_copy_queue, listener_copy_handler);
    });

    // Create our I/O thread pool
    let thread_pool = ThreadPool::new(io_threads);
    unsafe { io_pool = Box::into_raw(Box::new(thread_pool)) };

    // Startup the eventloop
    loop {
        event_loop(new_conn_pool.clone(), handler.clone());
    }
}

fn listen<A, S>(addr: A,
                new_conns: NewConnectionQueue<S>,
                mut handler: Handler<S>) where
    A: ToSocketAddrs,
    S: Stream
{
    let listener = TcpListener::bind(addr).unwrap();

    unsafe { (*handler.inner).on_server_created(&listener); }

    for result in listener.incoming() {
        match result {
            Ok(tcp_stream) => {
                let connection = unsafe { (*handler.inner).on_new_connection(tcp_stream) };
                let mut queue = new_conns.lock().unwrap();
                queue.push(connection);
            }
            Err(e) => warn!("Accepting incoming connection: {}", e)
        }
    }
}

fn event_loop<S: Stream>(new_conns: NewConnectionQueue<S>, handler: Handler<S>) {
    // Create our action queues
    let register_list = Arc::new(Mutex::new(Vec::<Interest>::new()));
    let modify_list = Arc::new(Mutex::new(Vec::<Interest>::new()));
    let remove_list = Arc::new(Mutex::new(Vec::<Interest>::new()));

    // Create our I/O batch areas
    let rx_queue = Arc::new(Mutex::new(Vec::<Interest>::new()));
    let tx_queue = Arc::new(Mutex::new(Vec::<Interest>::new()));

    // Spawn our I/O Sentinel
    let rx = rx_queue.clone();
    let tx = tx_queue.clone();
    let md_list = modify_list.clone();
    let rm_list = remove_list.clone();
    // thread::spawn(move || io_sentinel());


    // Create our Epoll Instance
    let epoll_instance = EpollInstance::new().unwrap();
    unsafe { epoll = Box::into_raw(Box::new(epoll_instance)); }



    loop {
        // Remove stale interests
        let mut remove = Vec::<Interest>::new();
        lock_n_swap_mem(&*remove_list, &mut remove);
        remove_interests(remove);

        // Add new interests
        let mut register = Vec::<Interest>::new();
        lock_n_swap_mem(&*register_list, &mut register);
        register_interests(register, &remove_list);

        // Modify owned interests
        let mut modify = Vec::<Interest>::new();
        lock_n_swap_mem(&*modify_list, &mut modify);


        // Wait
        match wait(5, num_io_threads()) {
            Ok(events) => {}
            Err(e) => {
                error!("During epoll wait: {}", e);
                break;
            }
        }
    }

    unsafe { ptr::drop_in_place(epoll); }
}

fn event_batcher<S: Stream>(mut interests: Vec<Interest>,
                            handler: &Handler<S>,
                            modify_list: &InterestList,
                            remove_list: &InterestList)
{
    for mut interest in interests.drain(..) {
        let event_flags = interest.events();

        if event_flags.contains(EPOLLERR)
            || event_flags.contains(EPOLLHUP)
            || event_flags.contains(EPOLLRDHUP)
        {
            add_interest_to_list(interest, remove_list);
            continue;
        }

        if event_flags.contains(EPOLLIN) {

        }

        if event_flags.contains(EPOLLOUT) {

        }
    }
}

fn io_sentinel<S: Stream>(connections: ConnectionSlab<S>,
                          handler: Handler<S>,
                          rx_queue: InterestList,
                          tx_queue: InterestList,
                          modify_list: InterestList,
                          remove_list: InterestList)
{
    loop {
        let mut queue = Vec::<Interest>::new();
        lock_n_swap_mem(&*rx_queue, &mut queue);
        for interest in queue.drain(..) {
            let maybe_connection = get_connection_from_interest(&interest, &connections);
            if maybe_connection.is_none() {
                add_interest_to_list(interest, &remove_list);
                continue;
            }
        }
    }
}

fn remove_interests(mut interests: Vec<Interest>) {
    for ref interest in interests.drain(..) {
        let _ = remove_interest(interest).map_err(|e| {
            warn!("Error removing interest: {}", e);
        });
    }
}

fn register_interests(mut interests: Vec<Interest>, removal_list: &InterestList) {
    for interest in interests.drain(..) {
        let _ = register_interest(interest.clone()).map_err(|e| {
            debug!("Error registering interest: {}", e);
            add_interest_to_list(interest, removal_list);
        });
    }
}

fn modify_interests(mut interests: Vec<Interest>, removal_list: &InterestList) {
    for ref interest in interests.drain(..) {
        let _ = modify_interest(interest).map_err(|e| {
            debug!("Error moodifying interest: {}", e);
            add_interest_to_list(interest.clone(), removal_list);
        });
    }
}

fn remove_interest(interest: &Interest) -> io::Result<()> {
    unsafe { (*epoll).del_interest(interest) }
}

fn register_interest(interest: Interest) -> io::Result<()> {
    unsafe { (*epoll).add_interest(interest) }
}

fn modify_interest(interest: &Interest) -> io::Result<()> {
    unsafe { (*epoll).mod_interest(interest) }
}

fn wait(timeout: i32, max_returned: usize) -> io::Result<Vec<Interest>> {
    unsafe { (*epoll).wait(timeout, max_returned) }
}

fn add_interest_to_list(interest: Interest, list: &InterestList) {
    let mut l = list.lock().unwrap();
    l.push(interest);
}

fn get_connection_from_interest<S: Stream>(interest: &Interest,
                                           connections: &ConnectionSlab<S>)
                                           -> Option<Arc<Connection<S>>>
{
    let list = connections.lock().unwrap();
    for connection in list.iter() {
        if connection.as_raw_fd() == interest.as_raw_fd() {
            return Some(connection.clone());
        }
    }

    None
}

fn lock_n_swap_mem<T: Any>(mutex: &Mutex<T>, new: &mut T) {
    let mut old = mutex.lock().unwrap();
    mem::swap(&mut *old, new);
}

fn num_io_threads() -> usize {
    unsafe { (*io_pool).active_count() }
}
