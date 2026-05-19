cells = {1, 2};
fh = @sum;
ints = ones(1, 2, like=int64(7));
labels = string([1, 2, 3]);
logicals = true(1, 2);
meta = struct("name", "alpha");
numbers = [1, 2, 3];

cell_ok = isa(cells, "cell");
double_ok = isa(numbers, "double");
float_ok = isa(numbers, "float");
handle_ok = isa(fh, "function_handle");
int_integer_ok = isa(ints, "integer");
int_numeric_ok = isa(ints, "numeric");
logical_ok = isa(logicals, "logical");
logical_numeric_ok = isa(logicals, "numeric");
string_ok = isa(labels, "string");
struct_ok = isa(meta, "struct");

float_complex = isfloat(complex(1, 2));
float_numbers = isfloat(numbers);
float_ints = isfloat(ints);
int_numbers = isinteger(numbers);
int_scalars = isinteger(ints);
kind_fh = class(fh);
kind_ints = class(ints);
kind_labels = class(labels);
kind_logicals = class(logicals);
kind_meta = class(meta);
kind_numbers = class(numbers);

caught_id = "none";
caught_depth = 0;
caught_is_mexception = false;
caught_kind = "none";
caught_msg = "none";
caught_top = "none";
default_id = "none";
default_depth = 0;
default_msg = "none";
default_top = "none";
status = "none";

try
    error("MATC:Demo", "explicit boom");
    status = "missed";
catch err
    caught_id = err.identifier;
    caught_depth = numel(err.stack);
    caught_is_mexception = isa(err, "MException");
    caught_kind = class(err);
    caught_msg = err.message;
    caught_top = err.stack(1).name;
    status = "caught";
end

try
    error("fallback boom");
catch err2
    default_id = err2.identifier;
    default_depth = numel(err2.stack);
    default_msg = err2.message;
    default_top = err2.stack(1).name;
end
