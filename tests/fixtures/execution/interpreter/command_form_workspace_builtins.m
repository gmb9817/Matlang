alpha = 1;
beta = "ok";
format bank
x = pi;
token = tic
token_is_scalar = isscalar(token);
elapsed = toc(token);
elapsed_nonnegative = elapsed >= 0;
names = who
summary = whos
name_count = numel(names);
summary_count = numel(summary);
clear alpha beta token elapsed names summary
