x = [1, 2];
status = "none";
try
    y = x(3);
    status = "missed";
catch err
    status = "caught";
    message = err.message;
    code = err.identifier;
end
