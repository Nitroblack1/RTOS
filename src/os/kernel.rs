use crate::os::task::{Task, TaskState};

pub struct Kernel {
    tasks: [Option<Task>; 3],
    current: usize,
}

impl Kernel {
    pub fn new() -> Self {
        Kernel {
            tasks: [None, None, None],
            current: 0,
        }
    }

    pub fn add_task(&mut self, task: Task) {
        for slot in self.tasks.iter_mut() {
            if slot.is_none() {
                *slot = Some(task);
                break;
            }
        }
    }

    pub fn run(&mut self) -> ! {
        loop {
            for task_opt in self.tasks.iter_mut() {
                if let Some(task) = task_opt {
                    if task.state == TaskState::Ready {
                        // context switch 기능 넣을 예정.
                        (task.entry_point)();
                    }
                }
            }
        }
    }
}
