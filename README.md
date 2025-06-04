# Lock-Free MPSC Queue
! THIS IS UNDER DEVELOPMENT AND TESTING

A high-performance, lock-free, bounded & unbounded multiple-producer single-consumer (MPSC) queue implemented in Rust. This project is designed for concurrent environments, leveraging atomic operations and a global backoff mechanism to handle contention efficiently.


---

## Features

- **Lock-Free Design**: Ensures high performance and scalability in multi-threaded environments.
- **Bounded Queue**: Implements a fixed-capacity queue to prevent unbounded memory usage.
- **Global Backoff**: Reduces contention using an exponential backoff strategy. For more details, [refer to the backoff implementation](https://github.com/LOKESH-999/lock-free-mpsc/blob/main/src/backoff.rs).
- **Cache Optimization**: Uses cache-line padding to minimize false sharing and improve performance. For more details, [refer to the backoff implementation](http://github.com/LOKESH-999/lock-free-mpsc/blob/main/src/cache_padded.rs).

---

## Repository Structure
*lock-free-mpsc*

    ├── src/
    │   ├── backoff.rs          # Global backoff mechanism for contention management
    │   ├── cache_padded.rs     # Cache-line padding for atomic variables
    │   ├── lib.rs              # Library entry point
    │   ├── main.rs             # Example binary entry point
    │   └── mpsc/
    │       ├── bounded_mpsc/
    │       │   ├── raw_mpsc.rs # Core implementation of the lock-free MPSC queue
    │       │   ├── slot.rs     # Individual slot management for the queue
    │       │   └── slot_arr.rs # Array of slots for queue storage

## How It Works

### Core Components

1. **`RawMpsc`**:
   - Implements the main queue logic.
   - Uses atomic operations for thread-safe access.
   - Relies on `SlotArr` for managing slots and `CachePadded` for avoiding cache-line contention.

2. **`GlobalBackoff`**:
   - Handles contention by introducing delays between retries.
   - Uses an exponential backoff strategy to reduce CPU usage under contention.

3. **`SlotArr` and `Slot`**:
   - Manage the storage(storage of data) and state of individual slots in the queue.

4. **`CachePadded`**:
   - Ensures atomic variables are aligned to cache-line boundaries to prevent false sharing.
   - Used in `RawMpsc` to wrap `next_head` and `tail`, ensuring they reside in separate cache lines.
   - Improves performance in multi-threaded environments by reducing cache contention.

---

## Global Backoff Workflow

The `GlobalBackoff` mechanism is used to manage contention during operations like `push` in the `RawMpsc` queue:

1. **Registration**: A thread registers itself for backoff using [`reg_wait`](src/backoff.rs).
2. **Waiting**: If contention occurs, the thread calls [`wait`](src/backoff.rs) to perform a spin-loop delay proportional to the contention level.
3. **Deregistration**: Once the operation succeeds or fails, the thread deregisters itself using [`de_reg`](src/backoff.rs).

### Key Methods in `GlobalBackoff`

- **[`new`](src/backoff.rs)**: Creates a new `GlobalBackoff` instance.
- **[`reg_wait`](src/backoff.rs)**: Registers the calling thread for backoff and performs an initial wait.
- **[`wait`](src/backoff.rs)**: Introduces a delay proportional to the current contention level.
- **[`de_reg`](src/backoff.rs)**: Deregisters the calling thread from contention tracking.
- **[`spin_for`](src/backoff.rs)**: Performs a specific number of spin iterations using `std::hint::spin_loop()`.
