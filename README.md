# sync-stack

Provides a synchronisation primitive simmilar to a Semaphore in `SyncStack`.

The `SyncStack` is a useful when implementing other synchronisation primitives in place of a wait loop; allowing threads to wait for some resource to become available by calling `SyncStack::park` and be notified when another thread calls `SyncStack::pop` later.

Author --- daniel.bechaz@gmail.com  
Last Moddified --- 2019-06-14
