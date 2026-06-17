{ lib }:

let
  isEmptyAttrs = value: builtins.isAttrs value && value == { };

  filterNulls =
    value:
    if builtins.isAttrs value then
      lib.filterAttrs (_name: child: child != null && !isEmptyAttrs child) (
        lib.mapAttrs (_name: child: filterNulls child) value
      )
    else if builtins.isList value then
      builtins.map filterNulls (builtins.filter (child: child != null) value)
    else
      value;
in
{
  inherit filterNulls;
}
