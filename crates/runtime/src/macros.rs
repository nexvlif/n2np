#[macro_export]
macro_rules! winapi_api {
    (
        $dll:literal,
        $name:literal,
        $vis:vis unsafe extern "win64" fn $fn_name:ident($($arg:ident: $ty:ty),* $(,)?)
        $body:block
    ) => {
        $vis unsafe extern "win64" fn $fn_name($($arg: $ty),*) {
            $crate::trace::record($dll, $name);
            $body
        }
    };
    (
        $dll:literal,
        $name:literal,
        $vis:vis unsafe extern "win64" fn $fn_name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty
        $body:block
    ) => {
        $vis unsafe extern "win64" fn $fn_name($($arg: $ty),*) -> $ret {
            $crate::trace::record($dll, $name);
            $body
        }
    };
    (
        $dll:literal,
        $name:literal,
        $vis:vis unsafe extern "win64" fn $fn_name:ident($($arg:ident: $ty:ty),* $(,)?) -> !
        $body:block
    ) => {
        $vis unsafe extern "win64" fn $fn_name($($arg: $ty),*) -> ! {
            $crate::trace::record($dll, $name);
            $body
        }
    };
}
