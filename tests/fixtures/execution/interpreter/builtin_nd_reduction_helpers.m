a = reshape(1:8, [2 2 2]);
sum_default = sum(a);
prod_default = prod(a);
sum_dim3 = sum(a, 3);
prod_dim3 = prod(a, 3);
max_default = max(a);
min_default = min(a);
[max_dim3, max_idx_dim3] = max(a, [], 3);
[min_dim3, min_idx_dim3] = min(a, [], 3);
cumsum_dim3 = cumsum(a, 3);
cumprod_dim3 = cumprod(a, 3);
[cummax_dim3, cummax_idx_dim3] = cummax(a, 3);
[cummin_dim3, cummin_idx_dim3] = cummin(a, 3);
var_dim3 = var(a, 0, 3);
std_dim3 = std(a, 0, 3);

logic = reshape([true false true false], [2 1 2]);
logic_sum = sum(logic, 3);
logic_cumsum = cumsum(logic, 3);

logic_mask = reshape([1 0 1 0 1 1 0 0], [2 2 2]);
any_dim3 = any(logic_mask, 3);
all_dim3 = all(logic_mask, 3);
