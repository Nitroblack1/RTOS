#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct TaskId(pub u8);

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum TaskState {
    Ready,
    Blocked,
    Suspended,
}

pub struct Task {
    pub id: TaskId,
    pub state: TaskState,
    pub entry_point: fn(),
}

impl Task {
    pub fn new(id: TaskId, entry: fn()) -> Self {
        Self {
            id,
            state: TaskState::Ready,
            entry_point: entry,
        }
    }
}
