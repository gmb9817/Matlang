vec_cov = cov([1 3 5]);
vec_cov_pop = cov([1 3 5], 1);
pair_cov = cov([1 2 3], [2 4 6]);
mat_cov = cov([1 2; 3 5; 5 6]);

vec_corr = corrcoef([1 3 5]);
pair_corr = corrcoef([1 2 3], [2 4 6]);
mat_corr = corrcoef([1 2; 3 5; 5 6]);
