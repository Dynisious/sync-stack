//! Provides a synchronisation primitive simmilar to a Semaphore in [SyncStack].
//! 
//! Calling `SyncStack::park` will park the current thread on top of the stack where it
//! will wait until it has been popped off the stack by a call to `SyncStack::pop`.
//! 
//! Author --- daniel.bechaz@gmail.com  
//! Last Moddified --- 2019-06-14

#![no_std]

#[cfg(any(test, feature = "std",),)]
extern crate std;

use core::{
  ptr,
  marker::{Send, Sync,},
  sync::atomic::{self, AtomicPtr, AtomicBool, Ordering,},
};

/// A stack of blocked threads.
pub struct SyncStack(AtomicPtr<SyncStackNode>,);

impl SyncStack {
  /// An empty `SyncStack`.
  pub const INIT: Self = SyncStack(AtomicPtr::new(ptr::null_mut(),),);

  /// Returns `Self::INIT`.
  #[inline]
  pub const fn new() -> Self { Self::INIT }
  /// Attempts to block the current thread on the top of the `SyncStack`.
  /// 
  /// Returns `true` if this thread was blocked and then unblocked.
  /// 
  /// Note that the `Park` implementation used does not have to be the same for every
  /// call to `park`; as such different thread implementations can all wait on the same
  /// `SyncStack`.
  /// 
  /// ```rust
  /// use sync_stack::*;
  /// # use std::{thread, time::Duration,};
  /// #
  /// # struct Thread(thread::Thread,);
  /// #
  /// # unsafe impl Park for Thread {
  /// #   #[inline]
  /// #   fn new() -> Self { Thread(thread::current(),) }
  /// #   #[inline]
  /// #   fn park() { thread::park() }
  /// #   #[inline]
  /// #   fn unpark(&self,) { self.0.unpark() }
  /// # }
  /// 
  /// static STACK: SyncStack = SyncStack::INIT;
  /// 
  /// std::thread::spawn(move || {
  ///   //This threads execution stops.
  ///   STACK.park::<Thread>();
  ///   println!("Ran Second");
  /// });
  /// 
  /// println!("Ran First");
  /// 
  /// //The other thread resumes execution.
  /// STACK.pop();
  /// ```
  pub fn park<P,>(&self,) -> bool
    where P: Park, {
    let park = P::new();
    //The node for this thread on the sync stack.
    let mut node = SyncStackNode {
      used: AtomicBool::new(false,),
      unpark: &mut move || park.unpark(),
      rest: self.0.load(Ordering::Relaxed,),
    };
    
    //Attempt to update the current pointer.
    if self.0.compare_and_swap(node.rest, &mut node, Ordering::AcqRel,) == node.rest {
      //Pointer updated, park thread until its popped from the stack.
      while !node.used.load(Ordering::SeqCst,) {
        P::park();
      }

      //Unparked, return
      true
    } else { false }
  }
  /// Unblocks a thread from the `SyncStack`.
  /// 
  /// Returns `false` if the stack was empty.
  pub fn pop(&self,) -> bool {
    //Get the node on the top of the stack.
    let mut node_ptr = self.0.load(Ordering::Acquire,);

    loop {
      //Confirm that the stack is not empty.
      if node_ptr == ptr::null_mut() { return false }

      let node = unsafe { &mut *node_ptr };

      //Update the stack before modifying the other thread in any way.
      let rest = node.rest;
      let new_node = AtomicPtr::new(self.0.compare_and_swap(node_ptr, rest, Ordering::Release,),);

      atomic::fence(Ordering::Release,);
      //Confirm that we successfuly own this node.
      if new_node.load(Ordering::Relaxed,) == node_ptr {
        atomic::fence(Ordering::Acquire,);
        if !node.used.compare_and_swap(false, true, Ordering::Release,) {
          atomic::fence(Ordering::SeqCst,);
          //Unpark the thread.
          unsafe { (*node.unpark)(); }

          return true;
        }
      } else {
        //Try again with the latest node.
        node_ptr = new_node.load(Ordering::Relaxed,);
      }
    }
  }
}

/// A node in a `SyncStack`.
struct SyncStackNode {
  used: AtomicBool,
  /// The thread to wake.
  unpark: *mut dyn FnMut(),
  /// The rest of the `SyncStack`.
  rest: *mut Self,
}

/// An handle used to unpark a thread.
/// 
/// Note that `thread` need not mean `std::thread::Thread` but could be any number of
/// user/kernal thread implementations.
/// 
/// An implementation for `std::thread::Thread` is available behind the `std` feature.
pub unsafe trait Park: 'static + Send + Sync {
  /// Returns a handle to unpark the current thread.
  fn new() -> Self;
  /// Parks the current thread when called.
  /// 
  /// # Safety
  /// 
  /// To avoid deadlocks occouring it is important that in the following execution order
  /// this function exists immediatly.
  /// 
  /// - thread1 start
  /// - thread2 start
  /// - thread1 pass unpark handle to thread2
  /// - thread2 unparks thread1
  /// - thread1 attempts to park
  fn park();
  /// Unparks the thread handled by this instance when called.
  /// 
  /// See [park](#method.park) documentation for details.
  fn unpark(&self,);
}

#[cfg(any(test, feature = "std",))]
unsafe impl Park for std::thread::Thread {
  #[inline]
  fn new() -> Self { std::thread::current() }
  #[inline]
  fn park() { std::thread::park() }
  #[inline]
  fn unpark(&self,) { self.unpark() }
}

#[cfg(test,)]
mod tests {
  use super::*;
  use std::{
    sync::{Mutex, Arc,},
    thread::{self, Thread,},
    time::Duration,
  };

  #[test]
  fn test_sync_stack_data_race() {
    static STACK: SyncStack = SyncStack::new();
    
    const THREADS_HALF: u64 = 1000;
    const CHAOS: u64 = 10;
    const CYCLES: u64 = 5;
    const THREADS: u64 = THREADS_HALF + THREADS_HALF;
    const SLEEP: u64 = 500;

    //A count of how many threads finished successfully.
    let finished = Arc::new(Mutex::new(0,),);

    for _ in 0..THREADS_HALF {
      let finished1 = finished.clone();
      thread::spawn(move || {
        for _ in 0..CYCLES {
          while !STACK.park::<Thread>() {};
          for _ in 0..CHAOS { STACK.pop(); }
        }

        *finished1.lock().unwrap() += 1;
      });

      let finished1 = finished.clone();
      thread::spawn(move || {
        for _ in 0..CYCLES {
          for _ in 0..CHAOS { STACK.pop(); }
          while !STACK.park::<Thread>() {};
        }
        
        *finished1.lock().unwrap() += 1;
      });
    }

    thread::sleep(Duration::from_millis(SLEEP,),);

    //Wait for all work to finish or progress to stop occouring.
    loop {
      let mut old_finished = 0;

      while {
        let finished = *finished.lock().unwrap();
        let sleep = finished != THREADS
          && finished != old_finished;
        
        old_finished = finished;

        sleep
      } {
        thread::sleep(Duration::from_millis(SLEEP,),);
      }

      if !STACK.pop() { break }
    }

    //Confirm all threads finished.
    assert_eq!(*finished.lock().unwrap(), THREADS,);
  }
}
