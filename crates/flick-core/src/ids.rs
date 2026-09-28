//! Stable identifiers used across Flick storage, APIs and events.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ulid::Ulid;

macro_rules! ulid_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Ulid);

        impl $name {
            /// Creates a new monotonic-ish ULID backed by the current system time.
            #[must_use]
            pub fn new() -> Self {
                Self(Ulid::generate())
            }

            /// Returns the inner ULID value.
            #[must_use]
            pub const fn as_ulid(self) -> Ulid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = ulid::DecodeError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Ulid::from_str(value).map(Self)
            }
        }

        impl From<Ulid> for $name {
            fn from(value: Ulid) -> Self {
                Self(value)
            }
        }
    };
}

ulid_newtype!(/// A configured camera identifier.
CameraId);
ulid_newtype!(/// A Home Assistant instance identifier.
HaInstanceId);
ulid_newtype!(/// A taught place identifier.
PlaceId);
ulid_newtype!(/// A taught anchor identifier.
AnchorId);
ulid_newtype!(/// A mapping identifier.
MappingId);
ulid_newtype!(/// A gesture sample identifier.
GestureSampleId);
ulid_newtype!(/// A capture session identifier.
CaptureSessionId);
ulid_newtype!(/// A motion take identifier.
MotionTakeId);
ulid_newtype!(/// A motion template identifier.
MotionTemplateId);
ulid_newtype!(/// A classifier model identifier.
ClassifierModelId);
ulid_newtype!(/// An activity log row identifier.
ActivityId);
ulid_newtype!(/// A gesture event identifier.
GestureEventId);
ulid_newtype!(/// A teach or realign session identifier.
TeachSessionId);
ulid_newtype!(/// A user gesture pack identifier.
GesturePackId);

/// A parse error for [`GestureId`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GestureIdParseError {
    /// The id did not match a known namespace.
    #[error("unknown gesture id namespace")]
    UnknownNamespace,
    /// The builtin id was not in the core catalog.
    #[error("unknown builtin gesture '{0}'")]
    UnknownBuiltin(String),
    /// The system id was not recognized.
    #[error("unknown system gesture '{0}'")]
    UnknownSystem(String),
    /// The custom or motion suffix was not a valid ULID.
    #[error("invalid gesture ULID: {0}")]
    InvalidUlid(String),
}

/// Built-in gesture identifiers from the core catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuiltinGesture {
    /// A closed fist.
    ClosedFist,
    /// An open palm.
    OpenPalm,
    /// The MediaPipe pointing-up label.
    PointingUp,
    /// A thumbs-up pose.
    ThumbUp,
    /// A thumbs-down pose.
    ThumbDown,
    /// A victory pose.
    Victory,
    /// The I-love-you pose.
    ILoveYou,
    /// Geometric point pose used for targeting.
    Point,
    /// A left swipe.
    SwipeLeft,
    /// A right swipe.
    SwipeRight,
    /// An upward swipe.
    SwipeUp,
    /// A downward swipe.
    SwipeDown,
    /// A continuous pinch dial.
    PinchDial,
    /// A clockwise circle.
    CircleCw,
    /// A counter-clockwise circle.
    CircleCcw,
    /// Either circle direction.
    CircleAny,
    /// The two-hand separate stop gesture.
    TwoHandSeparate,
}

impl BuiltinGesture {
    /// Returns the canonical suffix without the `builtin.` namespace.
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::ClosedFist => "closed_fist",
            Self::OpenPalm => "open_palm",
            Self::PointingUp => "pointing_up",
            Self::ThumbUp => "thumb_up",
            Self::ThumbDown => "thumb_down",
            Self::Victory => "victory",
            Self::ILoveYou => "i_love_you",
            Self::Point => "point",
            Self::SwipeLeft => "swipe_left",
            Self::SwipeRight => "swipe_right",
            Self::SwipeUp => "swipe_up",
            Self::SwipeDown => "swipe_down",
            Self::PinchDial => "pinch_dial",
            Self::CircleCw => "circle_cw",
            Self::CircleCcw => "circle_ccw",
            Self::CircleAny => "circle_any",
            Self::TwoHandSeparate => "two_hand_separate",
        }
    }
}

impl FromStr for BuiltinGesture {
    type Err = GestureIdParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "closed_fist" => Ok(Self::ClosedFist),
            "open_palm" => Ok(Self::OpenPalm),
            "pointing_up" => Ok(Self::PointingUp),
            "thumb_up" => Ok(Self::ThumbUp),
            "thumb_down" => Ok(Self::ThumbDown),
            "victory" => Ok(Self::Victory),
            "i_love_you" => Ok(Self::ILoveYou),
            "point" => Ok(Self::Point),
            "swipe_left" => Ok(Self::SwipeLeft),
            "swipe_right" => Ok(Self::SwipeRight),
            "swipe_up" => Ok(Self::SwipeUp),
            "swipe_down" => Ok(Self::SwipeDown),
            "pinch_dial" => Ok(Self::PinchDial),
            "circle_cw" => Ok(Self::CircleCw),
            "circle_ccw" => Ok(Self::CircleCcw),
            "circle_any" => Ok(Self::CircleAny),
            "two_hand_separate" => Ok(Self::TwoHandSeparate),
            other => Err(GestureIdParseError::UnknownBuiltin(other.to_owned())),
        }
    }
}

/// A gesture identifier: built-in, custom static, custom motion or system negative class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GestureId {
    /// A built-in catalog gesture.
    Builtin(BuiltinGesture),
    /// A user-trained static gesture.
    Custom(Ulid),
    /// A user-trained motion or two-hand template.
    Motion(Ulid),
    /// The internal negative class.
    SystemNone,
}

impl fmt::Display for GestureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(builtin) => write!(f, "builtin.{}", builtin.suffix()),
            Self::Custom(id) => write!(f, "custom.{id}"),
            Self::Motion(id) => write!(f, "motion.{id}"),
            Self::SystemNone => f.write_str("system.none"),
        }
    }
}

impl FromStr for GestureId {
    type Err = GestureIdParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(suffix) = value.strip_prefix("builtin.") {
            return suffix.parse().map(Self::Builtin);
        }
        if let Some(suffix) = value.strip_prefix("custom.") {
            return Ulid::from_str(suffix)
                .map(Self::Custom)
                .map_err(|err| GestureIdParseError::InvalidUlid(err.to_string()));
        }
        if let Some(suffix) = value.strip_prefix("motion.") {
            return Ulid::from_str(suffix)
                .map(Self::Motion)
                .map_err(|err| GestureIdParseError::InvalidUlid(err.to_string()));
        }
        if let Some(suffix) = value.strip_prefix("system.") {
            return match suffix {
                "none" => Ok(Self::SystemNone),
                other => Err(GestureIdParseError::UnknownSystem(other.to_owned())),
            };
        }
        Err(GestureIdParseError::UnknownNamespace)
    }
}

impl Serialize for GestureId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for GestureId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}
