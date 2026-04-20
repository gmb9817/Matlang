nums = reshape(1:8, [2 2 2]);
sum_handle = @sum;
num_out = sum_handle(nums(:, end));

texts = ["aa" "bbb"; "c" "dddd"];
text_handle = @strlength;
text_out = text_handle(texts(:, end));
