use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum MouseButton {
    Left = 1,
    Right = 2,
    Middle = 3,
    Button4 = 4,
    Button5 = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum KeyAction {
    Down = 1,
    Up = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum MouseAction {
    Down = 1,
    Up = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardEvent {
    pub scancode: u32,
    pub action: KeyAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MouseEvent {
    Move {
        dx: i32,
        dy: i32,
    },
    Button {
        button: MouseButton,
        action: MouseAction,
    },
    Scroll {
        delta_y: i32,
        delta_x: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputEvent {
    Keyboard(KeyboardEvent),
    Mouse(MouseEvent),
}
