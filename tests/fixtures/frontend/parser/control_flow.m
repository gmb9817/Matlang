function y = classify(x)
if x < 0
    y = -1;
elseif x < 10
    y = x;
else
    y = 10;
end
end
