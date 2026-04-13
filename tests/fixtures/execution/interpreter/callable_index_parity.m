values = cat(3, [1 2; 3 4], [5 6; 7 8]);
index_tail = values(1, 2:end);
index_last = values(:, end);

fh = @sum;
handle_idx = values(1, fh(1:end-2));
err_id = '';
try
    fh(2:end)
catch err
    err_id = err.identifier;
    clear err
end
